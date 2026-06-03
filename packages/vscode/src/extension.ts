import * as vscode from 'vscode';
import { ChatPanel } from './chatPanel';
import { HyperClient } from './hyperClient';

let hyperClient: HyperClient | undefined;
let chatPanel: ChatPanel | undefined;

export function activate(context: vscode.ExtensionContext) {
  console.log('HyperAgent extension activating...');

  // Initialize MCP client connecting to hyper mcp-server
  hyperClient = new HyperClient();
  chatPanel = new ChatPanel(context, hyperClient);

  // Register commands
  context.subscriptions.push(
    vscode.commands.registerCommand('hyperagent.openChat', () => {
      chatPanel?.show();
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('hyperagent.runPrompt', async () => {
      const prompt = await vscode.window.showInputBox({
        prompt: 'Enter a prompt for HyperAgent',
        placeHolder: 'e.g. add error handling to the login function',
        ignoreFocusOut: true,
      });
      if (prompt) {
        chatPanel?.show();
        chatPanel?.sendPrompt(prompt);
      }
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('hyperagent.explainCode', async () => {
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
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('hyperagent.fixErrors', async () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor) return;
      const uri = editor.document.uri;
      chatPanel?.show();
      chatPanel?.sendPrompt(`Check ${uri.fsPath} for errors and fix them`);
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('hyperagent.generateTests', async () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor) return;
      const text = editor.document.getText(editor.selection);
      const uri = editor.document.uri;
      chatPanel?.show();
      chatPanel?.sendPrompt(
        `Generate tests for ${uri.fsPath}${text ? `:\n\`\`\`\n${text}\n\`\`\`` : ''}`
      );
    })
  );

  // Auto-start chat on first install
  if (!context.globalState.get('hyperagent.seen')) {
    context.globalState.update('hyperagent.seen', true);
    vscode.commands.executeCommand('hyperagent.openChat');
  }
}

export function deactivate() {
  hyperClient?.dispose();
  chatPanel?.dispose();
}
