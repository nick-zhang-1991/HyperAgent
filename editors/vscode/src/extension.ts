import * as vscode from 'vscode';
import { execSync, exec } from 'child_process';
import { promisify } from 'util';

const execAsync = promisify(exec);
const OUTPUT_NAME = 'HyperAgent';

function config(): vscode.WorkspaceConfiguration {
    return vscode.workspace.getConfiguration('hyperagent');
}

function checkHyperInstalled(): boolean {
    try {
        execSync('hyper --version', { timeout: 5000, stdio: 'ignore' });
        return true;
    } catch {
        return false;
    }
}

function getServerUrl(): string {
    return config().get<string>('serverUrl', 'http://127.0.0.1:3000');
}

function getTerminal(): vscode.Terminal {
    const term = vscode.window.terminals.find(t => t.name === OUTPUT_NAME);
    return term || vscode.window.createTerminal(OUTPUT_NAME);
}

function sendToTerminal(cmd: string) {
    const term = getTerminal();
    term.show();
    term.sendText(cmd, true);
}

async function checkServerHealth(): Promise<boolean> {
    try {
        const url = `${getServerUrl()}/api/health`;
        const resp = await fetch(url, { signal: AbortSignal.timeout(3000) });
        return resp.ok;
    } catch {
        return false;
    }
}

class ChatPanel {
    public static currentPanel: ChatPanel | undefined;
    private readonly _panel: vscode.WebviewPanel;
    private _disposables: vscode.Disposable[] = [];

    private constructor(panel: vscode.WebviewPanel) {
        this._panel = panel;
        this._panel.onDidDispose(() => this.dispose(), null, this._disposables);
        this._panel.webview.html = this._getHtmlContent();
        this._panel.webview.onDidReceiveMessage(
            msg => this._handleMessage(msg),
            null,
            this._disposables
        );
    }

    public static createOrShow() {
        const column = vscode.window.activeTextEditor
            ? vscode.window.activeTextEditor.viewColumn
            : undefined;

        if (ChatPanel.currentPanel) {
            ChatPanel.currentPanel._panel.reveal(column);
            return;
        }

        const panel = vscode.window.createWebviewPanel(
            'hyperagentChat',
            'HyperAgent Chat',
            column || vscode.ViewColumn.Beside,
            { enableScripts: true }
        );

        ChatPanel.currentPanel = new ChatPanel(panel);
    }

    private async _handleMessage(msg: any) {
        switch (msg.type) {
            case 'sendMessage':
                await this._sendChatMessage(msg.text);
                break;
            case 'checkHealth':
                const healthy = await checkServerHealth();
                this._panel.webview.postMessage({
                    type: 'healthResult',
                    healthy,
                    url: getServerUrl()
                });
                break;
        }
    }

    private async _sendChatMessage(text: string) {
        const url = `${getServerUrl()}/api/chat`;
        try {
            const resp = await fetch(url, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ message: text })
            });
            if (!resp.ok) {
                this._panel.webview.postMessage({
                    type: 'error',
                    text: `Server error: ${resp.status}`
                });
                return;
            }
            const data = await resp.json();
            this._panel.webview.postMessage({
                type: 'response',
                text: data.response,
                sessionId: data.session_id
            });
        } catch (e: any) {
            this._panel.webview.postMessage({
                type: 'error',
                text: `Connection failed: ${e.message || e}. Is \`hyper serve\` running?`
            });
        }
    }

    private _getHtmlContent(): string {
        return `<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; background: var(--vscode-editor-background); color: var(--vscode-editor-foreground); }
        #header { padding: 12px 16px; border-bottom: 1px solid var(--vscode-panel-border); display: flex; align-items: center; gap: 8px; }
        #header h2 { font-size: 14px; font-weight: 600; }
        #status { font-size: 11px; padding: 2px 8px; border-radius: 4px; }
        .connected { background: #1b5e20; color: #a5d6a7; }
        .disconnected { background: #b71c1c; color: #ef9a9a; }
        #chat { padding: 12px 16px; height: calc(100vh - 120px); overflow-y: auto; }
        .msg { margin-bottom: 12px; }
        .msg-user { text-align: right; }
        .msg-assistant { text-align: left; }
        .msg-bubble { display: inline-block; padding: 8px 12px; border-radius: 8px; max-width: 85%; font-size: 13px; line-height: 1.5; white-space: pre-wrap; }
        .msg-user .msg-bubble { background: var(--vscode-button-background); color: var(--vscode-button-foreground); }
        .msg-assistant .msg-bubble { background: var(--vscode-editor-inactiveSelectionBackground); }
        #input-area { position: fixed; bottom: 0; left: 0; right: 0; padding: 12px 16px; border-top: 1px solid var(--vscode-panel-border); background: var(--vscode-editor-background); display: flex; gap: 8px; }
        #input { flex: 1; padding: 8px 12px; border: 1px solid var(--vscode-input-border); border-radius: 4px; background: var(--vscode-input-background); color: var(--vscode-input-foreground); font-size: 13px; outline: none; }
        #input:focus { border-color: var(--vscode-focusBorder); }
        #send { padding: 8px 16px; background: var(--vscode-button-background); color: var(--vscode-button-foreground); border: none; border-radius: 4px; cursor: pointer; font-size: 13px; }
        #send:hover { background: var(--vscode-button-hoverBackground); }
        #send:disabled { opacity: 0.5; cursor: default; }
        #welcome { text-align: center; padding: 40px 20px; color: var(--vscode-descriptionForeground); }
        #welcome h3 { margin-bottom: 8px; }
        #welcome p { font-size: 13px; line-height: 1.6; }
        #welcome code { background: var(--vscode-textCodeBlock-background); padding: 2px 6px; border-radius: 3px; }
    </style>
</head>
<body>
    <div id="header">
        <h2>🤖 HyperAgent</h2>
        <span id="status" class="disconnected">disconnected</span>
    </div>
    <div id="chat">
        <div id="welcome">
            <h3>Welcome to HyperAgent</h3>
            <p>Ask coding questions, get code reviews, or run tasks.<br>
            Make sure <code>hyper serve</code> is running first.</p>
        </div>
    </div>
    <div id="input-area">
        <input type="text" id="input" placeholder="Ask HyperAgent..." />
        <button id="send" disabled>Send</button>
    </div>
    <script>
        (function() {
            const vscode = acquireVsCodeApi();
            const chat = document.getElementById('chat');
            const input = document.getElementById('input');
            const sendBtn = document.getElementById('send');
            const status = document.getElementById('status');
            const welcome = document.getElementById('welcome');

            // Check health on load
            vscode.postMessage({ type: 'checkHealth' });

            function addMessage(role, text) {
                if (welcome) welcome.style.display = 'none';
                const div = document.createElement('div');
                div.className = 'msg msg-' + role;
                const bubble = document.createElement('div');
                bubble.className = 'msg-bubble';
                bubble.textContent = text;
                div.appendChild(bubble);
                chat.appendChild(div);
                chat.scrollTop = chat.scrollHeight;
            }

            function sendMessage() {
                const text = input.value.trim();
                if (!text) return;
                addMessage('user', text);
                input.value = '';
                sendBtn.disabled = true;
                input.disabled = true;
                vscode.postMessage({ type: 'sendMessage', text });
            }

            input.addEventListener('keydown', e => {
                if (e.key === 'Enter' && !e.shiftKey) {
                    e.preventDefault();
                    sendMessage();
                }
            });

            sendBtn.addEventListener('click', sendMessage);

            input.addEventListener('input', () => {
                sendBtn.disabled = !input.value.trim();
            });

            window.addEventListener('message', event => {
                const msg = event.data;
                switch (msg.type) {
                    case 'response':
                        addMessage('assistant', msg.text);
                        sendBtn.disabled = false;
                        input.disabled = false;
                        input.focus();
                        break;
                    case 'error':
                        addMessage('assistant', '⚠️ ' + msg.text);
                        sendBtn.disabled = false;
                        input.disabled = false;
                        break;
                    case 'healthResult':
                        status.textContent = msg.healthy ? 'connected (' + msg.url + ')' : 'disconnected';
                        status.className = msg.healthy ? 'connected' : 'disconnected';
                        break;
                }
            });
        })();
    </script>
</body>
</html>`;
    }

    public dispose() {
        ChatPanel.currentPanel = undefined;
        this._panel.dispose();
        while (this._disposables.length) {
            const d = this._disposables.pop();
            if (d) d.dispose();
        }
    }
}

export function activate(context: vscode.ExtensionContext) {
    const output = vscode.window.createOutputChannel(OUTPUT_NAME);
    context.subscriptions.push(output);

    // Status bar
    const statusItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
    statusItem.text = '$(symbol-event) HyperAgent';
    statusItem.tooltip = 'HyperAgent — Ultra-fast CLI coding agent';
    statusItem.command = 'hyperagent.run';
    const installed = checkHyperInstalled();
    if (installed) {
        statusItem.backgroundColor = undefined;
    } else {
        statusItem.text = '$(warning) HyperAgent (not found)';
        statusItem.tooltip = 'Install hyper CLI: cargo install hyperagent';
    }
    statusItem.show();
    context.subscriptions.push(statusItem);

    // Auto-start server
    if (installed && config().get<boolean>('autoStartServer', false)) {
        getTerminal().sendText('hyper serve &', true);
    }

    // Commands
    const runTask = vscode.commands.registerCommand('hyperagent.run', async () => {
        const prompt = await vscode.window.showInputBox({
            prompt: 'What coding task should HyperAgent execute?',
            placeHolder: 'e.g. add rate limiting to API gateway',
            ignoreFocusOut: true
        });
        if (!prompt) return;
        sendToTerminal(`hyper run '${prompt.replace(/'/g, "'\\''")}'`);
    });

    const chatPanel = vscode.commands.registerCommand('hyperagent.chat', () => {
        ChatPanel.createOrShow();
    });

    const review = vscode.commands.registerCommand('hyperagent.review', () => {
        sendToTerminal('hyper review');
    });

    const search = vscode.commands.registerCommand('hyperagent.search', async () => {
        const query = await vscode.window.showInputBox({
            prompt: 'Search the web with HyperAgent',
            placeHolder: 'e.g. Rust async best practices',
            ignoreFocusOut: true
        });
        if (!query) return;
        sendToTerminal(`hyper search '${query.replace(/'/g, "'\\''")}'`);
    });

    const init = vscode.commands.registerCommand('hyperagent.init', () => {
        sendToTerminal('hyper init');
    });

    const doctor = vscode.commands.registerCommand('hyperagent.doctor', () => {
        output.clear();
        output.show();
        execAsync('hyper doctor', { timeout: 30000 })
            .then(({ stdout }) => output.appendLine(stdout))
            .catch((e: any) => output.appendLine(`[error] ${e.message || e}`));
    });

    const session = vscode.commands.registerCommand('hyperagent.session', () => {
        sendToTerminal('hyper session list');
    });

    const serverStatus = vscode.commands.registerCommand('hyperagent.serverStatus', async () => {
        const healthy = await checkServerHealth();
        if (healthy) {
            vscode.window.showInformationMessage(`HyperAgent server connected at ${getServerUrl()}`);
        } else {
            const action = await vscode.window.showWarningMessage(
                'HyperAgent server not running',
                'Start Server',
                'Show Terminal'
            );
            if (action === 'Start Server') {
                sendToTerminal('hyper serve');
            }
        }
    });

    context.subscriptions.push(
        runTask, chatPanel, review, search, init, doctor, session, serverStatus
    );

    output.appendLine('✓ HyperAgent extension v0.2.0 activated');
    if (!installed) {
        output.appendLine('⚠️  hyper CLI not found. Install: cargo install hyperagent');
    }
}

export function deactivate() {}
