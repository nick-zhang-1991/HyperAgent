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
exports.activate = activate;
exports.deactivate = deactivate;
const vscode = __importStar(require("vscode"));
const chatPanel_1 = require("./chatPanel");
const hyperClient_1 = require("./hyperClient");
let hyperClient;
let chatPanel;
function activate(context) {
    console.log('HyperAgent extension activating...');
    // Initialize MCP client connecting to hyper mcp-server
    hyperClient = new hyperClient_1.HyperClient();
    chatPanel = new chatPanel_1.ChatPanel(context, hyperClient);
    // Register commands
    context.subscriptions.push(vscode.commands.registerCommand('hyperagent.openChat', () => {
        chatPanel?.show();
    }));
    context.subscriptions.push(vscode.commands.registerCommand('hyperagent.runPrompt', async () => {
        const prompt = await vscode.window.showInputBox({
            prompt: 'Enter a prompt for HyperAgent',
            placeHolder: 'e.g. add error handling to the login function',
            ignoreFocusOut: true,
        });
        if (prompt) {
            chatPanel?.show();
            chatPanel?.sendPrompt(prompt);
        }
    }));
    context.subscriptions.push(vscode.commands.registerCommand('hyperagent.explainCode', async () => {
        const editor = vscode.window.activeTextEditor;
        if (!editor) {
            vscode.window.showErrorMessage('No active editor');
            return;
        }
        const selection = editor.selection;
        const text = editor.document.getText(selection);
        if (!text) {
            vscode.window.showErrorMessage('No code selected');
            return;
        }
        chatPanel?.show();
        chatPanel?.sendPrompt(`Explain this code:\n\`\`\`\n${text}\n\`\`\``);
    }));
    context.subscriptions.push(vscode.commands.registerCommand('hyperagent.fixErrors', async () => {
        const editor = vscode.window.activeTextEditor;
        if (!editor)
            return;
        const uri = editor.document.uri;
        chatPanel?.show();
        chatPanel?.sendPrompt(`Check ${uri.fsPath} for errors and fix them`);
    }));
    context.subscriptions.push(vscode.commands.registerCommand('hyperagent.generateTests', async () => {
        const editor = vscode.window.activeTextEditor;
        if (!editor)
            return;
        const text = editor.document.getText(editor.selection);
        const uri = editor.document.uri;
        chatPanel?.show();
        chatPanel?.sendPrompt(`Generate tests for ${uri.fsPath}${text ? `:\n\`\`\`\n${text}\n\`\`\`` : ''}`);
    }));
    // Auto-start chat on first install
    if (!context.globalState.get('hyperagent.seen')) {
        context.globalState.update('hyperagent.seen', true);
        vscode.commands.executeCommand('hyperagent.openChat');
    }
}
function deactivate() {
    hyperClient?.dispose();
    chatPanel?.dispose();
}
//# sourceMappingURL=extension.js.map