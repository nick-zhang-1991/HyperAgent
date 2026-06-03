"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
exports.ChatPanel = void 0;
const vscode = __importStar(require("vscode"));
/**
 * Webview-based chat panel for HyperAgent.
 */
class ChatPanel {
    panel;
    context;
    client;
    constructor(context, client) {
        this.context = context;
        this.client = client;
    }
    show() {
        if (this.panel) {
            this.panel.reveal(vscode.ViewColumn.Beside);
            return;
        }
        this.panel = vscode.window.createWebviewPanel('hyperagentChat', 'HyperAgent Chat', { viewColumn: vscode.ViewColumn.Beside, preserveFocus: true }, {
            enableScripts: true,
            retainContextWhenHidden: true,
            localResourceRoots: [vscode.Uri.joinPath(this.context.extensionUri, 'media')],
        });
        this.panel.webview.html = this.getHtml();
        this.panel.onDidDispose(() => {
            this.panel = undefined;
        });
        // Handle messages from the webview
        this.panel.webview.onDidReceiveMessage(async (message) => {
            if (message.type === 'prompt') {
                await this.handlePrompt(message.text);
            }
        }, undefined, this.context.subscriptions);
    }
    async sendPrompt(prompt) {
        this.show();
        // Send the prompt to the webview first
        this.postMessage({ type: 'userMessage', text: prompt });
        await this.handlePrompt(prompt);
    }
    async handlePrompt(prompt) {
        this.postMessage({ type: 'status', text: '🔄 Processing...' });
        try {
            const response = await this.client.sendPrompt(prompt);
            this.postMessage({ type: 'aiResponse', text: response });
        }
        catch (err) {
            this.postMessage({ type: 'error', text: `Error: ${err.message}` });
        }
    }
    postMessage(msg) {
        this.panel?.webview.postMessage(msg);
    }
    getHtml() {
        return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<style>
  :root {
    --bg: #1e1e2e;
    --surface: #2a2a3e;
    --border: #3a3a5e;
    --text: #e0e0f0;
    --text2: #9090b0;
    --accent: #5b8def;
    --green: #3acf7a;
    --red: #e74c5e;
    --code-bg: #1a1a2e;
  }
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
    background: var(--bg);
    color: var(--text);
    height: 100vh;
    display: flex;
    flex-direction: column;
  }
  .header {
    background: var(--surface);
    border-bottom: 1px solid var(--border);
    padding: 10px 16px;
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    font-weight: 600;
  }
  .header span { color: var(--accent); }
  #messages {
    flex: 1;
    overflow-y: auto;
    padding: 12px 16px;
  }
  .msg { margin-bottom: 12px; }
  .msg-user { color: var(--accent); font-weight: 500; font-size: 12px; }
  .msg-user-text {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 8px 12px;
    margin-top: 4px;
    font-size: 13px;
    white-space: pre-wrap;
  }
  .msg-ai { color: var(--green); font-weight: 500; font-size: 12px; }
  .msg-ai-text {
    margin-top: 4px;
    font-size: 13px;
    line-height: 1.5;
    white-space: pre-wrap;
  }
  .msg-ai-text code {
    background: var(--code-bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 8px 12px;
    display: block;
    font-family: 'SF Mono', Monaco, monospace;
    font-size: 12px;
    overflow-x: auto;
    margin: 8px 0;
  }
  .msg-error { color: var(--red); }
  .msg-status { color: var(--text2); font-style: italic; font-size: 12px; }
  .input-area {
    background: var(--surface);
    border-top: 1px solid var(--border);
    padding: 10px 16px;
    display: flex;
    gap: 8px;
  }
  .input-area textarea {
    flex: 1;
    background: var(--bg);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 8px 12px;
    font-family: inherit;
    font-size: 13px;
    resize: none;
    outline: none;
    min-height: 36px;
  }
  .input-area textarea:focus { border-color: var(--accent); }
  .input-area button {
    background: var(--accent);
    color: white;
    border: none;
    border-radius: 6px;
    padding: 8px 16px;
    font-size: 13px;
    cursor: pointer;
    font-weight: 500;
  }
  .input-area button:hover { opacity: 0.85; }
</style>
</head>
<body>
  <div class="header">⚡ <span>HyperAgent</span> Chat</div>
  <div id="messages"></div>
  <div class="input-area">
    <textarea id="input" rows="1" placeholder="Ask HyperAgent to code..."></textarea>
    <button id="send">Send</button>
  </div>

<script>
  const vscode = acquireVsCodeApi();
  const messages = document.getElementById('messages')!;
  const input = document.getElementById('input') as HTMLTextAreaElement;
  const sendBtn = document.getElementById('send')!;

  function addMessage(type, content) {
    const div = document.createElement('div');
    div.className = 'msg';

    if (type === 'userMessage') {
      div.innerHTML = '<div class="msg-user">You</div><div class="msg-user-text">' + escapeHtml(content) + '</div>';
    } else if (type === 'aiResponse') {
      div.innerHTML = '<div class="msg-ai">HyperAgent</div><div class="msg-ai-text">' + formatResponse(content) + '</div>';
    } else if (type === 'error') {
      div.innerHTML = '<div class="msg-error">' + escapeHtml(content) + '</div>';
    } else if (type === 'status') {
      div.innerHTML = '<div class="msg-status">' + escapeHtml(content) + '</div>';
    }

    messages.appendChild(div);
    messages.scrollTop = messages.scrollHeight;
  }

  function escapeHtml(s) {
    if (!s) return '';
    return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  }

  function formatResponse(text) {
    if (!text) return '';
    // Format code blocks
    return text.replace(/\\\`\\\`\\\`(\\w*)\\n?([\\s\\S]*?)\\\`\\\`\\\`/g, '<code>$2</code>')
               .replace(/\\n/g, '<br>');
  }

  // Handle messages from extension
  window.addEventListener('message', event => {
    const msg = event.data;
    addMessage(msg.type, msg.text);
  });

  function send() {
    const text = input.value.trim();
    if (!text) return;
    vscode.postMessage({ type: 'prompt', text });
    addMessage('userMessage', text);
    input.value = '';
    input.style.height = 'auto';
  }

  sendBtn.addEventListener('click', send);
  input.addEventListener('keydown', e => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  });
  input.addEventListener('input', () => {
    input.style.height = 'auto';
    input.style.height = Math.min(input.scrollHeight, 200) + 'px';
  });

  // Focus input
  input.focus();
</script>
</body>
</html>`;
    }
    dispose() {
        this.panel?.dispose();
    }
}
exports.ChatPanel = ChatPanel;
//# sourceMappingURL=chatPanel.js.map