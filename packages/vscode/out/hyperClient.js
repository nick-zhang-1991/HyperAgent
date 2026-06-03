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
exports.HyperClient = void 0;
const vscode = __importStar(require("vscode"));
const child_process_1 = require("child_process");
const path = __importStar(require("path"));
/**
 * MCP client that connects to `hyper mcp-server` via stdio.
 */
class HyperClient {
    process = null;
    requestId = 0;
    pending = new Map();
    buffer = '';
    constructor() {
        this.start();
    }
    start() {
        // Find the hyper binary
        const hyperPath = this.findHyperBinary();
        if (!hyperPath) {
            vscode.window.showErrorMessage('HyperAgent binary not found. Install with: cargo install hyperagent');
            return;
        }
        this.process = (0, child_process_1.spawn)(hyperPath, ['mcp-server'], {
            stdio: ['pipe', 'pipe', 'pipe'],
        });
        this.process.stdout?.on('data', (data) => {
            this.buffer += data.toString();
            this.processBuffer();
        });
        this.process.stderr?.on('data', (data) => {
            console.error('HyperAgent stderr:', data.toString());
        });
        this.process.on('exit', (code) => {
            console.log(`HyperAgent MCP server exited with code ${code}`);
            this.process = null;
            // Reject all pending requests
            for (const [, entry] of this.pending) {
                entry.reject(new Error('MCP server disconnected'));
            }
            this.pending.clear();
        });
        this.process.on('error', (err) => {
            console.error('HyperAgent MCP server error:', err);
        });
    }
    findHyperBinary() {
        // Check common locations
        const candidates = ['hyper', 'hyperagent'];
        for (const cmd of candidates) {
            try {
                const which = require('which');
                const p = which.sync(cmd);
                if (p)
                    return p;
            }
            catch {
                // Try next
            }
        }
        // Check common paths
        const home = process.env.HOME || '';
        const paths = [
            path.join(home, '.local', 'bin', 'hyper'),
            path.join(home, '.local', 'bin', 'hyperagent'),
            path.join(home, '.cargo', 'bin', 'hyper'),
        ];
        for (const p of paths) {
            const fs = require('fs');
            if (fs.existsSync(p))
                return p;
        }
        return null;
    }
    processBuffer() {
        const lines = this.buffer.split('\n');
        // Keep incomplete line in buffer
        this.buffer = lines.pop() || '';
        for (const line of lines) {
            if (!line.trim())
                continue;
            try {
                const msg = JSON.parse(line);
                if (msg.id !== undefined && this.pending.has(msg.id)) {
                    const entry = this.pending.get(msg.id);
                    this.pending.delete(msg.id);
                    if (msg.error) {
                        entry.reject(new Error(msg.error.message));
                    }
                    else {
                        entry.resolve(msg.result);
                    }
                }
            }
            catch {
                // Non-JSON output (e.g. log messages)
                console.log('HyperAgent:', line);
            }
        }
    }
    /**
     * Send a prompt to HyperAgent via MCP and get the response.
     */
    async sendPrompt(prompt) {
        const id = ++this.requestId;
        const request = {
            jsonrpc: '2.0',
            id,
            method: 'execute_prompt',
            params: { prompt },
        };
        return new Promise((resolve, reject) => {
            this.pending.set(id, {
                resolve: (result) => {
                    resolve(result?.response || result?.text || JSON.stringify(result));
                },
                reject,
            });
            if (this.process?.stdin) {
                this.process.stdin.write(JSON.stringify(request) + '\n');
            }
            else {
                this.pending.delete(id);
                reject(new Error('MCP server not running'));
            }
            // Timeout after 120s
            setTimeout(() => {
                if (this.pending.has(id)) {
                    this.pending.delete(id);
                    reject(new Error('Request timed out after 120s'));
                }
            }, 120_000);
        });
    }
    /**
     * Search the codebase via MCP.
     */
    async searchCode(query) {
        const id = ++this.requestId;
        const request = {
            jsonrpc: '2.0',
            id,
            method: 'search_code',
            params: { query },
        };
        return new Promise((resolve, reject) => {
            this.pending.set(id, {
                resolve: (result) => resolve(JSON.stringify(result, null, 2)),
                reject,
            });
            if (this.process?.stdin) {
                this.process.stdin.write(JSON.stringify(request) + '\n');
            }
            else {
                this.pending.delete(id);
                reject(new Error('MCP server not running'));
            }
        });
    }
    dispose() {
        if (this.process) {
            this.process.kill();
            this.process = null;
        }
        this.pending.clear();
    }
}
exports.HyperClient = HyperClient;
//# sourceMappingURL=hyperClient.js.map