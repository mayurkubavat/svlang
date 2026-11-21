import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as vscode from 'vscode';
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
    Executable
} from 'vscode-languageclient/node';

// Single unified server (`svlang lsp`, pure-Rust IEEE 1800-2023 front end):
// live lint diagnostics and quick fixes, outline, workspace symbols, go to
// definition, references, hover, and completion.
const clients: LanguageClient[] = [];
const watchers: vscode.Disposable[] = [];
const warned = new Set<string>();

const documentSelector = [
    { scheme: 'file', language: 'systemverilog' },
    { scheme: 'file', language: 'verilog' }
];

export async function activate(context: vscode.ExtensionContext) {
    context.subscriptions.push(
        vscode.commands.registerCommand('svlang.restartServer', async () => {
            warned.clear();
            await stopAll();
            startAll(context);
            vscode.window.showInformationMessage('svlang language server restarted.');
        }),
        vscode.workspace.onDidChangeConfiguration(async e => {
            if (e.affectsConfiguration('svlang') || e.affectsConfiguration('svlang-lint')) {
                await stopAll();
                startAll(context);
            }
        })
    );
    startAll(context);
}

function getSetting<T>(
    primary: vscode.WorkspaceConfiguration,
    legacy: vscode.WorkspaceConfiguration,
    key: string,
    legacyKey: string,
    fallback: T
): T {
    const inspected = primary.inspect<T>(key);
    const hasPrimary = inspected?.workspaceFolderValue !== undefined
        || inspected?.workspaceValue !== undefined
        || inspected?.globalValue !== undefined;
    if (hasPrimary) {
        return primary.get<T>(key, fallback);
    }
    const legInspected = legacy.inspect<T>(legacyKey);
    const hasLegacy = legInspected?.workspaceFolderValue !== undefined
        || legInspected?.workspaceValue !== undefined
        || legInspected?.globalValue !== undefined;
    if (hasLegacy) {
        return legacy.get<T>(legacyKey, fallback);
    }
    return primary.get<T>(key, fallback);
}

function startAll(context: vscode.ExtensionContext) {
    const config = vscode.workspace.getConfiguration('svlang');
    const legacy = vscode.workspace.getConfiguration('svlang-lint');
    const workspaceRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
    const watcher = vscode.workspace.createFileSystemWatcher('**/*.{sv,svh,v,vh,svi,svp}');
    // The project's config, waivers, and baseline: a change re-lints.
    const configWatcher = vscode.workspace.createFileSystemWatcher(
        '**/{.svlang-lint/*,svlang_lint_config.py,.svlang-lint.py,.svlang-lint.json,svlang-lint.json}'
    );
    watchers.push(watcher, configWatcher);

    const lint = getSetting<boolean>(config, legacy, 'enable', 'enable', true);
    const navigation = getSetting<boolean>(config, legacy, 'navigation.enable', 'navigation.enable', true);

    if (lint || navigation) {
        const rawServerPath = getSetting<string>(config, legacy, 'serverPath', 'navigation.serverPath', 'svlang').trim();
        // Treat the former `dv-lsp` default as the default bundled `svlang` binary.
        const serverPath = rawServerPath === 'dv-lsp' || rawServerPath === 'dv-lsp.exe' ? 'svlang' : rawServerPath;
        const exe = resolveExecutable(context, serverPath, 'svlang');
        if (exe) {
            const preset = getSetting<string>(config, legacy, 'preset', 'preset', '').trim();
            const initializationOptions: Record<string, unknown> = {
                lint,
                navigation,
                libraryPaths: getSetting<string[]>(config, legacy, 'navigation.libraryPaths', 'navigation.libraryPaths', []),
                exclude: getSetting<string[]>(config, legacy, 'navigation.exclude', 'navigation.exclude', []),
                useUvmHome: getSetting<boolean>(config, legacy, 'navigation.useUvmHome', 'navigation.useUvmHome', true)
            };
            if (preset) {
                initializationOptions.preset = preset;
            }
            const base = path.basename(exe).toLowerCase();
            const args = base === 'dv-lsp' || base === 'dv-lsp.exe' ? [] : ['lsp'];
            start(
                'svlang',
                'svlang',
                { command: exe, args, options: { env: process.env, cwd: workspaceRoot } },
                {
                    documentSelector,
                    synchronize: { fileEvents: [watcher, configWatcher] },
                    initializationOptions
                }
            );
        } else {
            missing('svlang', 'svlang.serverPath', 'linting and code browsing');
        }
    }
}

function start(id: string, name: string, exe: Executable, options: LanguageClientOptions) {
    const serverOptions: ServerOptions = { run: exe, debug: exe };
    const client = new LanguageClient(id, name, serverOptions, options);
    clients.push(client);
    client.start().catch(err => {
        vscode.window.showErrorMessage(`${name}: could not start ${exe.command}: ${err}`);
    });
}

function missing(binary: string, setting: string, what: string) {
    if (warned.has(binary)) {
        return;
    }
    warned.add(binary);
    vscode.window
        .showWarningMessage(
            `svlang: \`${binary}\` was not found, so ${what} is off. Install \`svlang\` or set ${setting}.`,
            'Open Settings'
        )
        .then(choice => {
            if (choice) {
                vscode.commands.executeCommand('workbench.action.openSettings', setting);
            }
        });
}

/**
 * Resolves the binary to run: an explicit configured path, else the binary
 * bundled with the extension for this platform (bin/<platform>-<arch>/<exe>),
 * else <exe> from PATH. Returns undefined when none exists.
 */
function resolveExecutable(context: vscode.ExtensionContext, configured: string, name: string): string | undefined {
    const isWin = process.platform === 'win32';
    const exe = isWin ? `${name}.exe` : name;
    if (configured && configured !== name && configured !== exe) {
        const expanded = configured.replace(/^~(?=[/\\]|$)/, os.homedir());
        const abs = path.isAbsolute(expanded)
            ? expanded
            : path.join(vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? '', expanded);
        if (isExecutable(abs)) {
            return abs;
        }
        if (isWin && !abs.toLowerCase().endsWith('.exe') && isExecutable(`${abs}.exe`)) {
            return `${abs}.exe`;
        }
        return onPath(expanded);
    }
    const bundled = context.asAbsolutePath(path.join('bin', `${process.platform}-${process.arch}`, exe));
    if (isExecutable(bundled)) {
        return bundled;
    }
    return onPath(exe);
}

function isExecutable(p: string): boolean {
    try {
        fs.accessSync(p, fs.constants.X_OK);
        return fs.statSync(p).isFile();
    } catch {
        return false;
    }
}

function onPath(exe: string): string | undefined {
    const isWin = process.platform === 'win32';
    const candidates = isWin && !exe.toLowerCase().endsWith('.exe') ? [exe, `${exe}.exe`] : [exe];
    for (const dir of (process.env.PATH ?? '').split(path.delimiter)) {
        if (!dir) {
            continue;
        }
        for (const cand of candidates) {
            const full = path.join(dir, cand);
            if (isExecutable(full)) {
                return full;
            }
        }
    }
    return undefined;
}

async function stopAll() {
    for (const w of watchers.splice(0)) {
        w.dispose();
    }
    const all = clients.splice(0);
    await Promise.all(all.map(c => c.stop().catch(() => undefined)));
}

export function deactivate(): Thenable<void> | undefined {
    return stopAll();
}
