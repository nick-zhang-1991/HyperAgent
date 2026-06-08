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
const child_process_1 = require("child_process");
const util_1 = require("util");
const execAsync = (0, util_1.promisify)(child_process_1.exec);
const OUTPUT_NAME = 'HyperAgent';
function checkHyperInstalled() {
    try {
        (0, child_process_1.execSync)('hyper --version', { timeout: 5000, stdio: 'ignore' });
        return true;
    }
    catch {
        return false;
    }
}
function getTerminal() {
    const term = vscode.window.terminals.find(t => t.name === OUTPUT_NAME);
    return term || vscode.window.createTerminal(OUTPUT_NAME);
}
function sendToTerminal(cmd) {
    const term = getTerminal();
    term.show();
    term.sendText(cmd, true);
}
function activate(context) {
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
    }
    else {
        statusItem.text = '$(warning) HyperAgent (not found)';
        statusItem.tooltip = 'Install hyper CLI: cargo install hyperagent';
    }
    statusItem.show();
    context.subscriptions.push(statusItem);
    // Commands
    const runTask = vscode.commands.registerCommand('hyperagent.run', async () => {
        const prompt = await vscode.window.showInputBox({
            prompt: 'What coding task should HyperAgent execute?',
            placeHolder: 'e.g. add rate limiting to API gateway',
            ignoreFocusOut: true
        });
        if (!prompt)
            return;
        sendToTerminal(`hyper run '${prompt.replace(/'/g, "'\\''")}'`);
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
        if (!query)
            return;
        sendToTerminal(`hyper search '${query.replace(/'/g, "'\\''")}'`);
    });
    const init = vscode.commands.registerCommand('hyperagent.init', () => {
        sendToTerminal('hyper init');
    });
    const commit = vscode.commands.registerCommand('hyperagent.commit', () => {
        sendToTerminal('hyper commit');
    });
    const doctor = vscode.commands.registerCommand('hyperagent.doctor', () => {
        sendToTerminal('hyper doctor');
    });
    const modeList = vscode.commands.registerCommand('hyperagent.modeList', async () => {
        output.clear();
        output.show();
        output.appendLine('$ hyper mode list');
        try {
            const { stdout } = await execAsync('hyper mode list', { timeout: 10000 });
            output.appendLine(stdout);
        }
        catch (e) {
            output.appendLine(`[error] ${e.message || e}`);
        }
    });
    context.subscriptions.push(runTask, review, search, init, commit, doctor, modeList);
    output.appendLine('✓ HyperAgent extension activated');
    if (!installed) {
        output.appendLine('⚠️  hyper CLI not found. Install with: cargo install hyperagent');
    }
}
function deactivate() { }
