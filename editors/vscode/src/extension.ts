import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
    Executable
} from 'vscode-languageclient/node';

// One server, `dv-lsp` (Rust, native front end): lint diagnostics and quick
// fixes, outline, symbols, go to definition, references, hover and
// completion. With `svlang-lint.executablePath` set, lint comes from that binary
// instead (`svlang-lint-slang --lsp`, the Slang reference engine).
const clients: LanguageClient[] = [];

const documentSelector = [
    { scheme: 'file', language: 'systemverilog' },
    { scheme: 'file', language: 'verilog' }
];

export async function activate(context: vscode.ExtensionContext) {
    context.subscriptions.push(
        vscode.commands.registerCommand('svlang-lint.restartServer', async () => {
            await stopAll();
            startAll(context);
            vscode.window.showInformationMessage('svlang-lint language server restarted.');
        }),
        vscode.workspace.onDidChangeConfiguration(async e => {
            if (e.affectsConfiguration('svlang-lint')) {
                await stopAll();
                startAll(context);
            }
        })
    );
    startAll(context);
}

function startAll(context: vscode.ExtensionContext) {
    const config = vscode.workspace.getConfiguration('svlang-lint');
    const workspaceRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
    const watcher = vscode.workspace.createFileSystemWatcher('**/*.{sv,svh,v,vh,svi,svp}');
    // The project's config, waivers and baseline: a change re-lints.
    const configWatcher = vscode.workspace.createFileSystemWatcher('**/{.svlang-lint/*,svlang_lint_config.py,.svlang-lint.py,.svlang-lint.json,svlang-lint.json}');
    context.subscriptions.push(watcher, configWatcher);

    const lint = config.get<boolean>('enable', true);
    const navigation = config.get<boolean>('navigation.enable', true);
    const slang = config.get<string>('executablePath', '').trim();
    const reference = lint && slang !== '' && slang !== 'svlang-lint';

    if (reference) {
        const exe = resolveExecutable(context, slang, 'svlang-lint-slang');
        if (exe) {
            start(context, 'svlang-lint-lsp', 'svlang-lint Lint (Slang)', { command: exe, args: ['--lsp'], options: { env: process.env, cwd: workspaceRoot } }, {
                documentSelector,
                synchronize: { fileEvents: watcher }
            });
        } else {
            missing(slang, 'svlang-lint.executablePath', 'lint with the Slang engine');
        }
    }

    const nativeLint = lint && !reference;
    if (nativeLint || navigation) {
        const exe = resolveExecutable(context, config.get<string>('navigation.serverPath', 'dv-lsp'), 'dv-lsp');
        if (exe) {
            start(context, 'dv-lsp', 'svlang-lint', { command: exe, args: [], options: { env: process.env, cwd: workspaceRoot } }, {
                documentSelector,
                synchronize: { fileEvents: [watcher, configWatcher] },
                initializationOptions: {
                    lint: nativeLint,
                    navigation,
                    libraryPaths: config.get<string[]>('navigation.libraryPaths', []),
                    exclude: config.get<string[]>('navigation.exclude', []),
                    useUvmHome: config.get<boolean>('navigation.useUvmHome', true)
                }
            });
        } else {
            missing('dv-lsp', 'svlang-lint.navigation.serverPath', 'linting and code browsing');
        }
    }
}

function start(context: vscode.ExtensionContext, id: string, name: string, exe: Executable, options: LanguageClientOptions) {
    const serverOptions: ServerOptions = { run: exe, debug: exe };
    const client = new LanguageClient(id, name, serverOptions, options);
    clients.push(client);
    client.start().catch(err => {
        vscode.window.showErrorMessage(`${name}: could not start ${exe.command}: ${err}`);
    });
}

const warned = new Set<string>();

function missing(binary: string, setting: string, what: string) {
    if (warned.has(binary)) {
        return;
    }
    warned.add(binary);
    vscode.window
        .showWarningMessage(`svlang-lint: \`${binary}\` was not found, so ${what} is off. Build it or set ${setting}.`, 'Open Settings')
        .then(choice => {
            if (choice) {
                vscode.commands.executeCommand('workbench.action.openSettings', setting);
            }
        });
}

/**
 * The binary to run: an explicit path from the settings, else the one bundled
 * with the extension for this platform (bin/<platform>-<arch>/<name>), else
 * <name> from PATH. Undefined when none exists.
 */
function resolveExecutable(context: vscode.ExtensionContext, configured: string, name: string): string | undefined {
    const exe = process.platform === 'win32' ? `${name}.exe` : name;
    if (configured && configured !== name) {
        const p = configured.replace(/^~(?=\/|$)/, process.env.HOME ?? '~');
        const abs = path.isAbsolute(p) ? p : path.join(vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? '', p);
        return isExecutable(abs) ? abs : onPath(p);
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
    for (const dir of (process.env.PATH ?? '').split(path.delimiter)) {
        if (dir && isExecutable(path.join(dir, exe))) {
            return path.join(dir, exe);
        }
    }
    return undefined;
}

async function stopAll() {
    const all = clients.splice(0);
    await Promise.all(all.map(c => c.stop().catch(() => undefined)));
}

export function deactivate(): Thenable<void> | undefined {
    return stopAll();
}
