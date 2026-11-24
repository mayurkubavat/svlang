import { execFile } from 'child_process';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as vscode from 'vscode';
import {
    Executable,
    LanguageClient,
    LanguageClientOptions,
    ServerOptions
} from 'vscode-languageclient/node';

// ---- Server & Status Types ----

interface ServerStatus {
    state: 'indexing' | 'linting' | 'ready' | 'syntax' | 'standalone' | 'off' | 'error';
    hasConfig?: boolean;
    configPath?: string;
    preset?: string;
    trigger?: string;
    errors?: number;
    warnings?: number;
    files?: number;
    symbols?: number;
}

interface LspPos {
    line: number;
    character: number;
}

interface LspRange {
    start: LspPos;
    end: LspPos;
}

interface RtlNode {
    instanceName: string;
    moduleName: string;
    kind: string;
    uri: string;
    range: LspRange;
    definitionUri: string;
    definitionRange: LspRange;
    children: RtlNode[];
}

interface TraceHit {
    uri: string;
    fileName: string;
    line: number;
    range: LspRange;
    lineText: string;
    container: string;
    kind: string;
}

interface TraceResult {
    symbol: {
        name: string;
        kind: string;
        detail: string;
        uri: string;
        range: LspRange;
    };
    declarations: TraceHit[];
    drivers: TraceHit[];
    loads: TraceHit[];
}

// ---- Module State ----

let client: LanguageClient | undefined;
let activeExePath: string | undefined;
let statusBar: vscode.StatusBarItem | undefined;
let inactiveDecoration: vscode.TextEditorDecorationType | undefined;
let currentOpacity = -1;
let latestStatus: ServerStatus = { state: 'syntax', trigger: 'off', preset: 'default' };
const watchers: vscode.Disposable[] = [];
const warned = new Set<string>();

let rtlProvider: RtlHierarchyProvider | undefined;
let traceProvider: SignalTraceProvider | undefined;

const SV_LANGUAGES = new Set(['systemverilog', 'verilog', 'sv-filelist']);

const documentSelector = [
    { scheme: 'file', language: 'systemverilog' },
    { scheme: 'file', language: 'verilog' }
];

const PRESETS: Array<{ label: string; value: string; description: string }> = [
    { label: 'Default (Config / Bug-Finding)', value: '', description: 'Use .svlang-lint/config.py preset (or soundness + methodology + performance + security)' },
    { label: 'formal-ready', value: 'formal-ready', description: 'Pre-FPV formal-readiness pack (SVA-FPV-001..005) and SVA checks' },
    { label: 'soundness', value: 'soundness', description: 'Correctness, concurrency, SVA, and type-safety bugs' },
    { label: 'methodology', value: 'methodology', description: 'UVM factory, phasing, objections, config_db, and TLM checks' },
    { label: 'performance', value: 'performance', description: 'Simulation and elaboration performance checks' },
    { label: 'style', value: 'style', description: 'Formatting, naming conventions, and end labels' },
    { label: 'maintainability', value: 'maintainability', description: 'Complexity, cohesion/coupling, and design smells' },
    { label: 'uvm-1800.2', value: 'uvm-1800.2', description: 'IEEE 1800.2 / UVM migration deprecation checks' },
    { label: 'reuse', value: 'reuse', description: 'UVM reuse guidelines (MG-1 through MG-5)' },
    { label: 'all', value: 'all', description: 'All non-experimental rules (including style, smells, and advisory)' }
];

// ---- Activation ----

export async function activate(context: vscode.ExtensionContext) {
    await vscode.commands.executeCommand('setContext', 'svlang.active', true);

    statusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
    statusBar.command = 'svlang.openMenu';
    context.subscriptions.push(statusBar);

    rtlProvider = new RtlHierarchyProvider();
    traceProvider = new SignalTraceProvider();
    context.subscriptions.push(
        vscode.window.registerTreeDataProvider('svlang.rtlHierarchy', rtlProvider),
        vscode.window.registerTreeDataProvider('svlang.signalTrace', traceProvider),
        vscode.languages.registerCodeActionsProvider(
            documentSelector,
            new IgnoreRuleCodeActionProvider(),
            { providedCodeActionKinds: [vscode.CodeActionKind.QuickFix] }
        )
    );

    registerCommands(context);

    context.subscriptions.push(
        vscode.window.onDidChangeActiveTextEditor(ed => {
            updateStatusBarVisibility();
            if (ed) {
                void refreshInactiveRegions(ed);
            }
        }),
        vscode.workspace.onDidChangeTextDocument(ev => {
            const ed = vscode.window.activeTextEditor;
            if (ed && ed.document === ev.document && isSvDocument(ev.document)) {
                void refreshInactiveRegions(ed);
            }
        }),
        vscode.workspace.onDidSaveTextDocument(doc => {
            if (isSvDocument(doc)) {
                rtlProvider?.refresh();
            }
        }),
        vscode.workspace.onDidChangeConfiguration(async e => {
            if (!e.affectsConfiguration('svlang') && !e.affectsConfiguration('svlang-lint')) {
                return;
            }
            if (e.affectsConfiguration('svlang.files.extraExtensions')) {
                await syncExtraExtensions();
            }
            if (e.affectsConfiguration('svlang.statusBar.show')) {
                updateStatusBarVisibility();
            }
            if (
                e.affectsConfiguration('svlang.inactiveRegions.enable') ||
                e.affectsConfiguration('svlang.inactiveRegions.opacity')
            ) {
                resetInactiveDecoration();
                await refreshAllVisibleInactiveRegions();
            }
            const needsServerRestart =
                e.affectsConfiguration('svlang.serverPath') ||
                e.affectsConfiguration('svlang.navigation') ||
                e.affectsConfiguration('svlang.semanticHighlighting') ||
                e.affectsConfiguration('svlang.trace.server');
            if (needsServerRestart) {
                await stopServer();
                await startServer(context);
            } else {
                await pushLiveLintOptions();
                await refreshAllVisibleInactiveRegions();
            }
        })
    );

    await syncExtraExtensions();
    await startServer(context);
    updateStatusBar();
    updateStatusBarVisibility();
}

// ---- Commands & Control Menu ----

function registerCommands(context: vscode.ExtensionContext) {
    context.subscriptions.push(
        vscode.commands.registerCommand('svlang.openMenu', () => showControlMenu(context)),
        vscode.commands.registerCommand('svlang.lintNow', async () => {
            if (!client) {
                await startServer(context);
            }
            if (client) {
                latestStatus = { ...latestStatus, state: 'linting' };
                updateStatusBar();
                await client.sendRequest('svlang/lintNow', {});
            }
        }),
        vscode.commands.registerCommand('svlang.switchTrigger', async () => {
            const cur = effectiveTrigger();
            const picked = await vscode.window.showQuickPick(
                [
                    { label: '$(zap) On Type', value: 'onType', description: cur === 'onType' ? 'Current — lint after typing pauses (250 ms)' : 'Lint after typing pauses (250 ms)' },
                    { label: '$(save) On Save', value: 'onSave', description: cur === 'onSave' ? 'Current — lint on file open and save' : 'Lint only when files are opened or saved' },
                    { label: '$(svlang-logo) Off (Browsing + Syntax/Semantics)', value: 'off', description: cur === 'off' ? 'Current — code browsing + IEEE 1800 syntax/semantics; Lint Now anytime' : 'Code browsing + IEEE 1800 syntax/semantics; run Lint Now anytime' }
                ],
                { placeHolder: 'Select when svlang runs automatic lint checks' }
            );
            if (!picked) {
                return;
            }
            const cfg = vscode.workspace.getConfiguration('svlang');
            if (picked.value !== 'off' && !cfg.get<boolean>('enable', true)) {
                await cfg.update('enable', true, configTarget());
            }
            await cfg.update('lint.trigger', picked.value, configTarget());
        }),
        vscode.commands.registerCommand('svlang.switchPreset', async () => {
            const cur = getSetting<string>('preset', 'preset', '');
            const picked = await vscode.window.showQuickPick(
                PRESETS.map(p => ({
                    label: p.value === cur ? `$(check) ${p.label}` : p.label,
                    description: p.description,
                    value: p.value
                })),
                { placeHolder: 'Select svlang lint preset' }
            );
            if (picked) {
                await vscode.workspace.getConfiguration('svlang').update('preset', picked.value, configTarget());
            }
        }),
        vscode.commands.registerCommand('svlang.setDefines', async () => {
            const cfg = vscode.workspace.getConfiguration('svlang');
            const current = cfg.get<string[]>('compile.defines', []);
            const raw = await vscode.window.showInputBox({
                title: 'svlang: Active Preprocessor Defines',
                prompt: 'Comma- or space-separated defines (e.g. SYNTHESIS, FORMAL, WIDTH=64). Leave empty to clear.',
                value: current.join(', ')
            });
            if (raw === undefined) {
                return;
            }
            const list = raw
                .split(/[,\s]+/)
                .map(s => s.trim())
                .filter(Boolean);
            await cfg.update('compile.defines', list, configTarget());
        }),
        vscode.commands.registerCommand('svlang.initConfig', () => runInitConfig(context)),
        vscode.commands.registerCommand('svlang.openConfig', () => openActiveConfig()),
        vscode.commands.registerCommand('svlang.openSettings', () => {
            void vscode.commands.executeCommand('workbench.action.openSettings', '@ext:svlang.svlang');
        }),
        vscode.commands.registerCommand('svlang.traceSignal', async () => {
            await traceSignalAtCursor();
        }),
        vscode.commands.registerCommand('svlang.refreshRtlHierarchy', () => {
            rtlProvider?.refresh();
        }),
        vscode.commands.registerCommand('svlang.ignoreRuleWorkspace', async (ruleId?: string) => {
            await addIgnoredRule(ruleId, vscode.ConfigurationTarget.Workspace);
        }),
        vscode.commands.registerCommand('svlang.ignoreRuleUser', async (ruleId?: string) => {
            await addIgnoredRule(ruleId, vscode.ConfigurationTarget.Global);
        }),
        vscode.commands.registerCommand('svlang.showOutput', () => {
            client?.outputChannel.show(true);
        }),
        vscode.commands.registerCommand('svlang.restartServer', async () => {
            warned.clear();
            await stopServer();
            await startServer(context);
            vscode.window.showInformationMessage('svlang language server restarted.');
        })
    );
}

async function showControlMenu(context: vscode.ExtensionContext) {
    const trig = effectiveTrigger();
    const preset = latestStatus.preset || getSetting<string>('preset', 'preset', '') || 'default';
    const defs = vscode.workspace.getConfiguration('svlang').get<string[]>('compile.defines', []);
    const hasCfg = Boolean(latestStatus.hasConfig || findWorkspaceConfigFile());

    interface ActionItem extends vscode.QuickPickItem {
        run?: () => Thenable<unknown> | void;
    }

    const items: ActionItem[] = [
        {
            label: '$(play) Lint Current File / Project Now',
            description: 'Run immediate check regardless of trigger mode',
            run: () => vscode.commands.executeCommand('svlang.lintNow')
        },
        {
            label: '$(zap) Set Lint Trigger...',
            description: `Current: ${trig}`,
            run: () => vscode.commands.executeCommand('svlang.switchTrigger')
        },
        {
            label: '$(sliders) Switch Lint Preset...',
            description: `Current: ${preset}`,
            run: () => vscode.commands.executeCommand('svlang.switchPreset')
        },
        {
            label: '$(symbol-constant) Set Preprocessor Defines...',
            description: defs.length ? defs.join(', ') : 'None',
            run: () => vscode.commands.executeCommand('svlang.setDefines')
        },
        {
            label: hasCfg ? '$(go-to-file) Open Active Project Config' : '$(add) Initialize Project Config (.svlang-lint/config.py)',
            description: hasCfg
                ? path.basename(latestStatus.configPath ?? findWorkspaceConfigFile() ?? 'config.py')
                : 'Auto-discover .f filelists, UVM, and house naming conventions',
            run: () => vscode.commands.executeCommand(hasCfg ? 'svlang.openConfig' : 'svlang.initConfig')
        },
        {
            label: '$(CallIncoming) Trace Signal Drivers & Loads at Cursor',
            description: 'Classify writers vs readers in Explorer -> Signal Trace',
            run: () => vscode.commands.executeCommand('svlang.traceSignal')
        },
        {
            label: '$(type-hierarchy) Refresh RTL Module Hierarchy',
            description: 'Update Explorer -> RTL Hierarchy',
            run: () => vscode.commands.executeCommand('svlang.refreshRtlHierarchy')
        },
        {
            label: '$(gear) Open svlang Settings',
            description: 'Configure global or workspace options',
            run: () => vscode.commands.executeCommand('svlang.openSettings')
        },
        {
            label: '$(output) Show Language Server Output',
            run: () => vscode.commands.executeCommand('svlang.showOutput')
        },
        {
            label: '$(refresh) Restart Language Server',
            run: async () => {
                warned.clear();
                await stopServer();
                await startServer(context);
            }
        }
    ];

    const picked = await vscode.window.showQuickPick(items, {
        title: `svlang Control Center (${latestStatus.files ?? 0} indexed files, ${latestStatus.symbols ?? 0} symbols)`,
        placeHolder: 'Choose an action'
    });
    if (picked?.run) {
        await picked.run();
    }
}

// ---- Language Server Lifecycle ----

async function startServer(context: vscode.ExtensionContext) {
    const config = vscode.workspace.getConfiguration('svlang');
    const workspaceRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
    const watcher = vscode.workspace.createFileSystemWatcher('**/*.{sv,svh,v,vh,svi,svp,f,F,vc}');
    const configWatcher = vscode.workspace.createFileSystemWatcher(
        '**/{.svlang-lint/*,svlang_lint_config.py,.svlang-lint.py,.svlang-lint.json,svlang-lint.json}'
    );
    watchers.push(watcher, configWatcher);

    const rawServerPath = getSetting<string>('serverPath', 'navigation.serverPath', 'svlang').trim();
    const serverPath = rawServerPath === 'dv-lsp' || rawServerPath === 'dv-lsp.exe' ? 'svlang' : rawServerPath;
    const exe = resolveExecutable(context, serverPath, 'svlang');
    activeExePath = exe;
    if (!exe) {
        missing('svlang', 'svlang.serverPath', 'linting and code browsing');
        latestStatus = { state: 'error' };
        updateStatusBar();
        return;
    }

    const lintOpts = collectLintOptions();
    const initializationOptions: Record<string, unknown> = {
        ...lintOpts,
        lint: lintOpts.trigger !== 'off',
        lintTrigger: lintOpts.trigger,
        navigation: getSetting<boolean>('navigation.enable', 'navigation.enable', true),
        semanticHighlighting: config.get<boolean>('semanticHighlighting.enable', true),
        libraryPaths: getSetting<string[]>('navigation.libraryPaths', 'navigation.libraryPaths', []),
        exclude: getSetting<string[]>('navigation.exclude', 'navigation.exclude', []),
        useUvmHome: getSetting<boolean>('navigation.useUvmHome', 'navigation.useUvmHome', true)
    };

    const base = path.basename(exe).toLowerCase();
    const args = base === 'dv-lsp' || base === 'dv-lsp.exe' ? [] : ['lsp'];
    const runExe: Executable = { command: exe, args, options: { env: process.env, cwd: workspaceRoot } };
    const serverOptions: ServerOptions = { run: runExe, debug: runExe };
    const clientOptions: LanguageClientOptions = {
        documentSelector,
        synchronize: { fileEvents: [watcher, configWatcher] },
        initializationOptions
    };

    client = new LanguageClient('svlang', 'svlang', serverOptions, clientOptions);
    client.onNotification('svlang/status', (st: ServerStatus) => {
        latestStatus = { ...latestStatus, ...st };
        updateStatusBar();
    });

    try {
        await client.start();
        const snap = await client.sendRequest<ServerStatus>('svlang/status', {});
        if (snap) {
            latestStatus = { ...latestStatus, ...snap };
        }
        updateStatusBar();
        rtlProvider?.refresh();
        await refreshAllVisibleInactiveRegions();
    } catch (err) {
        vscode.window.showErrorMessage(`svlang: could not start ${exe}: ${err}`);
        latestStatus = { state: 'error' };
        updateStatusBar();
    }
}

async function stopServer() {
    for (const w of watchers.splice(0)) {
        w.dispose();
    }
    if (client) {
        const c = client;
        client = undefined;
        await c.stop().catch(() => undefined);
    }
}

export function deactivate(): Thenable<void> | undefined {
    return stopServer();
}

// ---- Settings & Live Reconfiguration ----

function getSetting<T>(key: string, legacyKey: string, fallback: T): T {
    const primary = vscode.workspace.getConfiguration('svlang');
    const legacy = vscode.workspace.getConfiguration('svlang-lint');
    const inspected = primary.inspect<T>(key);
    if (
        inspected?.workspaceFolderValue !== undefined ||
        inspected?.workspaceValue !== undefined ||
        inspected?.globalValue !== undefined
    ) {
        return primary.get<T>(key, fallback);
    }
    const leg = legacy.inspect<T>(legacyKey);
    if (
        leg?.workspaceFolderValue !== undefined ||
        leg?.workspaceValue !== undefined ||
        leg?.globalValue !== undefined
    ) {
        return legacy.get<T>(legacyKey, fallback);
    }
    return primary.get<T>(key, fallback);
}

function effectiveTrigger(): 'onType' | 'onSave' | 'off' {
    const enabled = getSetting<boolean>('enable', 'enable', true);
    if (!enabled) {
        return 'off';
    }
    const raw = vscode.workspace.getConfiguration('svlang').get<string>('lint.trigger', 'off');
    if (raw === 'onType' || raw === 'onSave') {
        return raw;
    }
    return 'off';
}

function collectLintOptions() {
    const cfg = vscode.workspace.getConfiguration('svlang');
    const enabled = getSetting<boolean>('enable', 'enable', true);
    return {
        trigger: effectiveTrigger(),
        syntaxDiagnostics: enabled && cfg.get<boolean>('diagnostics.syntaxAndSemantics', true),
        preset: getSetting<string>('preset', 'preset', '').trim(),
        configPath: cfg.get<string>('lint.configPath', '').trim(),
        configPreference: cfg.get<string>('lint.configPreference', 'filesystemFirst'),
        requireConfig: cfg.get<boolean>('lint.requireConfig', false),
        select: cfg.get<string[]>('lint.select', []),
        ignore: cfg.get<string[]>('lint.ignore', []),
        filelists: cfg.get<string[]>('compile.filelists', []),
        defines: cfg.get<string[]>('compile.defines', []),
        includeDirs: cfg.get<string[]>('compile.includeDirs', [])
    };
}

async function pushLiveLintOptions() {
    if (!client) {
        return;
    }
    const opts = collectLintOptions();
    await client.sendRequest('svlang/setLintOptions', opts).catch(() => undefined);
    const snap = await client.sendRequest<ServerStatus>('svlang/status', {}).catch(() => undefined);
    if (snap) {
        latestStatus = { ...latestStatus, ...snap };
    }
    updateStatusBar();
}

function configTarget(): vscode.ConfigurationTarget {
    return vscode.workspace.workspaceFolders?.length
        ? vscode.ConfigurationTarget.Workspace
        : vscode.ConfigurationTarget.Global;
}

async function syncExtraExtensions() {
    const cfg = vscode.workspace.getConfiguration('svlang');
    const extras = cfg.get<string[]>('files.extraExtensions', []);
    if (!extras.length) {
        return;
    }
    const filesCfg = vscode.workspace.getConfiguration('files');
    const assoc = { ...(filesCfg.get<Record<string, string>>('associations') ?? {}) };
    let changed = false;
    for (const raw of extras) {
        const ext = raw.trim();
        if (!ext) {
            continue;
        }
        const pat = ext.startsWith('*.') ? ext : ext.startsWith('.') ? `*${ext}` : `*.${ext}`;
        if (assoc[pat] !== 'systemverilog') {
            assoc[pat] = 'systemverilog';
            changed = true;
        }
    }
    if (changed) {
        await filesCfg.update('associations', assoc, configTarget());
    }
}

// ---- Status Bar ----

function isSvDocument(doc: vscode.TextDocument | undefined): boolean {
    if (!doc) {
        return false;
    }
    if (SV_LANGUAGES.has(doc.languageId)) {
        return true;
    }
    const base = path.basename(doc.fileName);
    return base === 'svlang_lint_config.py' || doc.fileName.includes(`${path.sep}.svlang-lint${path.sep}`);
}

function updateStatusBarVisibility() {
    if (!statusBar) {
        return;
    }
    const mode = vscode.workspace.getConfiguration('svlang').get<string>('statusBar.show', 'whenSvOpen');
    if (mode === 'never') {
        statusBar.hide();
        return;
    }
    if (mode === 'always' || isSvDocument(vscode.window.activeTextEditor?.document)) {
        statusBar.show();
    } else {
        statusBar.hide();
    }
}

function updateStatusBar() {
    if (!statusBar) {
        return;
    }
    const st = latestStatus.state;
    const errs = latestStatus.errors ?? 0;
    const warns = latestStatus.warnings ?? 0;
    const preset =
        st !== 'syntax' && latestStatus.preset && latestStatus.preset !== 'default'
            ? ` · ${latestStatus.preset}`
            : '';

    if (st === 'indexing' || st === 'linting') {
        statusBar.text = `$(sync~spin) svlang`;
        statusBar.backgroundColor = undefined;
    } else if (st === 'error') {
        statusBar.text = `$(error) svlang`;
        statusBar.backgroundColor = new vscode.ThemeColor('statusBarItem.errorBackground');
    } else if (st === 'off') {
        statusBar.text = `$(circle-slash) svlang · off`;
        statusBar.backgroundColor = undefined;
    } else if (st === 'standalone') {
        statusBar.text = errs > 0 ? `$(error) svlang · ${errs}` : `$(svlang-logo) svlang · standalone`;
        statusBar.backgroundColor = undefined;
    } else if (errs > 0) {
        statusBar.text = `$(error) svlang · ${errs}${preset}`;
        statusBar.backgroundColor = undefined;
    } else if (warns > 0) {
        statusBar.text = `$(warning) svlang · ${warns}${preset}`;
        statusBar.backgroundColor = undefined;
    } else {
        statusBar.text = `$(svlang-logo) svlang${preset}`;
        statusBar.backgroundColor = undefined;
    }

    const defs = vscode.workspace.getConfiguration('svlang').get<string[]>('compile.defines', []);
    const tip = new vscode.MarkdownString(undefined, true);
    tip.isTrusted = true;
    tip.appendMarkdown(`**svlang** — IEEE 1800-2023 Language Server & Linter\n\n`);
    if (st === 'syntax') {
        tip.appendMarkdown(`- **Mode**: Browsing + Syntax/Semantics (\`auto-lint: off\`)\n`);
    } else {
        tip.appendMarkdown(`- **State**: \`${st}\` (trigger: \`${effectiveTrigger()}\`, preset: \`${latestStatus.preset || 'default'}\`)\n`);
    }
    tip.appendMarkdown(`- **Config**: ${latestStatus.hasConfig ? `\`${latestStatus.configPath ?? '.svlang-lint/config.py'}\`` : '_Standalone (no repo config)_'}\n`);
    if (defs.length) {
        tip.appendMarkdown(`- **Defines**: \`${defs.join(', ')}\`\n`);
    }
    tip.appendMarkdown(`\n[Lint Now](command:svlang.lintNow) · [Control Menu](command:svlang.openMenu) · [Settings](command:svlang.openSettings)`);
    statusBar.tooltip = tip;
}

// ---- Inactive Preprocessor (`ifdef) Dimming ----

function ensureInactiveDecoration(): vscode.TextEditorDecorationType {
    const opacity = vscode.workspace.getConfiguration('svlang').get<number>('inactiveRegions.opacity', 0.5);
    if (!inactiveDecoration || currentOpacity !== opacity) {
        inactiveDecoration?.dispose();
        currentOpacity = opacity;
        inactiveDecoration = vscode.window.createTextEditorDecorationType({
            opacity: String(Math.max(0.15, Math.min(1.0, opacity)))
        });
    }
    return inactiveDecoration;
}

function resetInactiveDecoration() {
    inactiveDecoration?.dispose();
    inactiveDecoration = undefined;
    currentOpacity = -1;
}

async function refreshInactiveRegions(editor: vscode.TextEditor) {
    if (!isSvDocument(editor.document) || editor.document.languageId === 'sv-filelist') {
        return;
    }
    const enabled = vscode.workspace.getConfiguration('svlang').get<boolean>('inactiveRegions.enable', true);
    if (!enabled) {
        if (inactiveDecoration) {
            editor.setDecorations(inactiveDecoration, []);
        }
        return;
    }
    if (!client) {
        return;
    }
    const deco = ensureInactiveDecoration();
    try {
        const raw = await client.sendRequest<LspRange[]>('svlang/inactiveRanges', {
            textDocument: { uri: editor.document.uri.toString() }
        });
        const ranges = (raw ?? []).map(
            r => new vscode.Range(r.start.line, r.start.character, r.end.line, r.end.character)
        );
        editor.setDecorations(deco, ranges);
    } catch {
        // Ignore transient requests during shutdown/restart.
    }
}

async function refreshAllVisibleInactiveRegions() {
    await Promise.all(vscode.window.visibleTextEditors.map(ed => refreshInactiveRegions(ed)));
}

// ---- One-Click Rule Ignore Code Actions ----

class IgnoreRuleCodeActionProvider implements vscode.CodeActionProvider {
    provideCodeActions(
        _document: vscode.TextDocument,
        _range: vscode.Range | vscode.Selection,
        context: vscode.CodeActionContext
    ): vscode.CodeAction[] {
        const out: vscode.CodeAction[] = [];
        const seen = new Set<string>();
        for (const diag of context.diagnostics) {
            if (diag.source !== 'svlang-lint') {
                continue;
            }
            const code = typeof diag.code === 'object' ? String(diag.code.value) : String(diag.code ?? '');
            if (!code || code === 'CMP-ERR-001' || code === 'CMP-ERR-002' || code === 'SVL-WVR-002' || seen.has(code)) {
                continue;
            }
            seen.add(code);

            if (vscode.workspace.workspaceFolders?.length) {
                const wsAction = new vscode.CodeAction(
                    `Ignore ${code} in Workspace Settings`,
                    vscode.CodeActionKind.QuickFix
                );
                wsAction.command = {
                    command: 'svlang.ignoreRuleWorkspace',
                    title: wsAction.title,
                    arguments: [code]
                };
                wsAction.diagnostics = [diag];
                out.push(wsAction);
            }

            const userAction = new vscode.CodeAction(
                `Ignore ${code} in User Settings`,
                vscode.CodeActionKind.QuickFix
            );
            userAction.command = {
                command: 'svlang.ignoreRuleUser',
                title: userAction.title,
                arguments: [code]
            };
            userAction.diagnostics = [diag];
            out.push(userAction);
        }
        return out;
    }
}

async function addIgnoredRule(ruleId: string | undefined, target: vscode.ConfigurationTarget) {
    const id = ruleId ?? (await vscode.window.showInputBox({ prompt: 'Rule ID to ignore (e.g. SVL-STY-104)' }))?.trim();
    if (!id) {
        return;
    }
    const cfg = vscode.workspace.getConfiguration('svlang');
    const existing = cfg.get<string[]>('lint.ignore', []);
    if (!existing.includes(id)) {
        await cfg.update('lint.ignore', [...existing, id], target);
    }
    if (latestStatus.hasConfig && cfg.get<string>('lint.configPreference', 'filesystemFirst') === 'filesystemFirst') {
        const choice = await vscode.window.showInformationMessage(
            `Added ${id} to svlang.lint.ignore. Because a repository config exists, switch configPreference to "editorFirst" if you want VS Code settings to override it.`,
            'Use editorFirst'
        );
        if (choice === 'Use editorFirst') {
            await cfg.update('lint.configPreference', 'editorFirst', target);
        }
    }
}

// ---- Project Config Init & Open ----

function findWorkspaceConfigFile(): string | undefined {
    const root = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
    if (!root) {
        return undefined;
    }
    for (const rel of [
        '.svlang-lint/config.py',
        '.svlang-lint/config.json',
        'svlang_lint_config.py',
        '.svlang-lint.py',
        '.svlang-lint.json',
        'svlang-lint.json'
    ]) {
        const full = path.join(root, rel);
        if (fs.existsSync(full)) {
            return full;
        }
    }
    return undefined;
}

async function openActiveConfig() {
    const cfgPath = latestStatus.configPath || findWorkspaceConfigFile();
    if (!cfgPath || !fs.existsSync(cfgPath)) {
        const choice = await vscode.window.showInformationMessage(
            'No .svlang-lint/config.py found in this workspace.',
            'Initialize Project Config'
        );
        if (choice) {
            await vscode.commands.executeCommand('svlang.initConfig');
        }
        return;
    }
    const doc = await vscode.workspace.openTextDocument(vscode.Uri.file(cfgPath));
    await vscode.window.showTextDocument(doc);
}

async function runInitConfig(context: vscode.ExtensionContext) {
    const root = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
    if (!root) {
        vscode.window.showWarningMessage('Open a workspace folder before initializing .svlang-lint/config.py.');
        return;
    }
    const exe = activeExePath ?? resolveExecutable(context, 'svlang', 'svlang');
    if (!exe) {
        missing('svlang', 'svlang.serverPath', 'project initialization');
        return;
    }
    await vscode.window.withProgress(
        { location: vscode.ProgressLocation.Notification, title: 'svlang: Initializing .svlang-lint/config.py...' },
        () =>
            new Promise<void>(resolve => {
                execFile(exe, ['lint', '--init'], { cwd: root }, async (err, _stdout, stderr) => {
                    if (err) {
                        vscode.window.showWarningMessage(`svlang lint --init: ${(stderr || err.message).trim()}`);
                    } else {
                        const created = path.join(root, '.svlang-lint', 'config.py');
                        if (fs.existsSync(created)) {
                            const doc = await vscode.workspace.openTextDocument(vscode.Uri.file(created));
                            await vscode.window.showTextDocument(doc);
                        }
                        await pushLiveLintOptions();
                        await vscode.commands.executeCommand('svlang.lintNow');
                    }
                    resolve();
                });
            })
    );
}

// ---- Explorer Sidebar Views: RTL Hierarchy & Signal Trace ----

class RtlTreeItem extends vscode.TreeItem {
    constructor(public readonly node: RtlNode) {
        const isRoot = node.instanceName === node.moduleName;
        const label = isRoot ? node.moduleName : `${node.instanceName} : ${node.moduleName}`;
        super(
            label,
            node.children.length
                ? vscode.TreeItemCollapsibleState.Expanded
                : vscode.TreeItemCollapsibleState.None
        );
        this.description = isRoot ? node.kind : undefined;
        this.iconPath = new vscode.ThemeIcon(node.kind === 'interface' ? 'plug' : isRoot ? 'chip' : 'symbol-module');
        const targetUri = vscode.Uri.parse(isRoot ? node.definitionUri : node.uri);
        const r = isRoot ? node.definitionRange : node.range;
        this.command = {
            command: 'vscode.open',
            title: 'Go to Module / Instance',
            arguments: [
                targetUri,
                { selection: new vscode.Range(r.start.line, r.start.character, r.end.line, r.end.character) }
            ]
        };
        this.tooltip = `${node.kind} ${node.moduleName}`;
    }
}

class RtlHierarchyProvider implements vscode.TreeDataProvider<vscode.TreeItem> {
    private readonly emitter = new vscode.EventEmitter<vscode.TreeItem | undefined | void>();
    readonly onDidChangeTreeData = this.emitter.event;

    refresh() {
        this.emitter.fire();
    }

    getTreeItem(element: vscode.TreeItem): vscode.TreeItem {
        return element;
    }

    async getChildren(element?: vscode.TreeItem): Promise<vscode.TreeItem[]> {
        if (element instanceof RtlTreeItem) {
            return element.node.children.map(c => new RtlTreeItem(c));
        }
        if (!client) {
            return [];
        }
        try {
            const res = await client.sendRequest<{ roots: RtlNode[] }>('svlang/rtlHierarchy', {});
            const roots = res?.roots ?? [];
            if (!roots.length) {
                const empty = new vscode.TreeItem('No RTL modules indexed in workspace');
                empty.iconPath = new vscode.ThemeIcon('info');
                return [empty];
            }
            return roots.map(r => new RtlTreeItem(r));
        } catch {
            return [];
        }
    }
}

class TraceGroupItem extends vscode.TreeItem {
    constructor(
        label: string,
        icon: string,
        public readonly hits: TraceHit[]
    ) {
        super(
            `${label} (${hits.length})`,
            hits.length ? vscode.TreeItemCollapsibleState.Expanded : vscode.TreeItemCollapsibleState.Collapsed
        );
        this.iconPath = new vscode.ThemeIcon(icon);
    }
}

class TraceHitItem extends vscode.TreeItem {
    constructor(hit: TraceHit) {
        super(`${hit.fileName}:${hit.line}`, vscode.TreeItemCollapsibleState.None);
        this.description = hit.lineText;
        this.tooltip = `${hit.container ? `${hit.container} — ` : ''}${hit.lineText}`;
        this.iconPath = new vscode.ThemeIcon(
            hit.kind === 'driver' || hit.kind === 'initializer'
                ? 'arrow-right'
                : hit.kind === 'declaration'
                    ? 'symbol-variable'
                    : 'arrow-left'
        );
        const r = hit.range;
        this.command = {
            command: 'vscode.open',
            title: 'Jump to Reference',
            arguments: [
                vscode.Uri.parse(hit.uri),
                { selection: new vscode.Range(r.start.line, r.start.character, r.end.line, r.end.character) }
            ]
        };
    }
}

class SignalTraceProvider implements vscode.TreeDataProvider<vscode.TreeItem> {
    private readonly emitter = new vscode.EventEmitter<vscode.TreeItem | undefined | void>();
    readonly onDidChangeTreeData = this.emitter.event;
    private current?: TraceResult;

    setTrace(trace: TraceResult | undefined) {
        this.current = trace;
        this.emitter.fire();
    }

    getTreeItem(element: vscode.TreeItem): vscode.TreeItem {
        return element;
    }

    getChildren(element?: vscode.TreeItem): vscode.TreeItem[] {
        if (element instanceof TraceGroupItem) {
            return element.hits.map(h => new TraceHitItem(h));
        }
        if (!this.current) {
            const hint = new vscode.TreeItem('Right-click a signal -> Trace Signal Drivers & Loads');
            hint.iconPath = new vscode.ThemeIcon('search');
            return [hint];
        }
        const header = new vscode.TreeItem(
            `${this.current.symbol.name} (${this.current.symbol.kind})`,
            vscode.TreeItemCollapsibleState.None
        );
        header.description = this.current.symbol.detail;
        header.iconPath = new vscode.ThemeIcon('symbol-field');
        const sr = this.current.symbol.range;
        header.command = {
            command: 'vscode.open',
            title: 'Go to Declaration',
            arguments: [
                vscode.Uri.parse(this.current.symbol.uri),
                { selection: new vscode.Range(sr.start.line, sr.start.character, sr.end.line, sr.end.character) }
            ]
        };
        return [
            header,
            new TraceGroupItem('Drivers / Writes', 'arrow-right', this.current.drivers),
            new TraceGroupItem('Loads / Reads', 'arrow-left', this.current.loads),
            new TraceGroupItem('Declarations', 'symbol-variable', this.current.declarations)
        ];
    }
}

async function traceSignalAtCursor() {
    const ed = vscode.window.activeTextEditor;
    if (!ed || !client) {
        return;
    }
    try {
        const res = await client.sendRequest<TraceResult | null>('svlang/traceSignal', {
            textDocument: { uri: ed.document.uri.toString() },
            position: { line: ed.selection.active.line, character: ed.selection.active.character }
        });
        if (!res) {
            vscode.window.showInformationMessage('Place the cursor on a signal, port, variable, or class property to trace.');
            return;
        }
        traceProvider?.setTrace(res);
        await vscode.commands.executeCommand('svlang.signalTrace.focus');
    } catch (err) {
        vscode.window.showWarningMessage(`svlang signal trace failed: ${err}`);
    }
}

// ---- Binary Discovery ----

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
                void vscode.commands.executeCommand('workbench.action.openSettings', setting);
            }
        });
}

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
