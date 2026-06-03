import * as vscode from 'vscode';
import { ChildProcess, spawn } from 'child_process';
import * as path from 'path';

interface McpResponse {
  result?: any;
  error?: { message: string };
}

/**
 * MCP client that connects to `hyper mcp-server` via stdio.
 */
export class HyperClient implements vscode.Disposable {
  private process: ChildProcess | null = null;
  private requestId = 0;
  private pending = new Map<number, { resolve: (v: any) => void; reject: (e: Error) => void }>();
  private buffer = '';

  constructor() {
    this.start();
  }

  private start() {
    // Find the hyper binary
    const hyperPath = this.findHyperBinary();
    if (!hyperPath) {
      vscode.window.showErrorMessage(
        'HyperAgent binary not found. Install with: cargo install hyperagent'
      );
      return;
    }

    this.process = spawn(hyperPath, ['mcp-server'], {
      stdio: ['pipe', 'pipe', 'pipe'],
    });

    this.process.stdout?.on('data', (data: Buffer) => {
      this.buffer += data.toString();
      this.processBuffer();
    });

    this.process.stderr?.on('data', (data: Buffer) => {
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

  private findHyperBinary(): string | null {
    // Check common locations
    const candidates = ['hyper', 'hyperagent'];
    for (const cmd of candidates) {
      try {
        const which = require('which');
        const p = which.sync(cmd);
        if (p) return p;
      } catch {
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
      if (fs.existsSync(p)) return p;
    }
    return null;
  }

  private processBuffer() {
    const lines = this.buffer.split('\n');
    // Keep incomplete line in buffer
    this.buffer = lines.pop() || '';

    for (const line of lines) {
      if (!line.trim()) continue;
      try {
        const msg = JSON.parse(line);
        if (msg.id !== undefined && this.pending.has(msg.id)) {
          const entry = this.pending.get(msg.id)!;
          this.pending.delete(msg.id);
          if (msg.error) {
            entry.reject(new Error(msg.error.message));
          } else {
            entry.resolve(msg.result);
          }
        }
      } catch {
        // Non-JSON output (e.g. log messages)
        console.log('HyperAgent:', line);
      }
    }
  }

  /**
   * Send a prompt to HyperAgent via MCP and get the response.
   */
  async sendPrompt(prompt: string): Promise<string> {
    const id = ++this.requestId;
    const request = {
      jsonrpc: '2.0',
      id,
      method: 'execute_prompt',
      params: { prompt },
    };

    return new Promise((resolve, reject) => {
      this.pending.set(id, {
        resolve: (result: any) => {
          resolve(result?.response || result?.text || JSON.stringify(result));
        },
        reject,
      });

      if (this.process?.stdin) {
        this.process.stdin.write(JSON.stringify(request) + '\n');
      } else {
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
  async searchCode(query: string): Promise<string> {
    const id = ++this.requestId;
    const request = {
      jsonrpc: '2.0',
      id,
      method: 'search_code',
      params: { query },
    };

    return new Promise((resolve, reject) => {
      this.pending.set(id, {
        resolve: (result: any) => resolve(JSON.stringify(result, null, 2)),
        reject,
      });

      if (this.process?.stdin) {
        this.process.stdin.write(JSON.stringify(request) + '\n');
      } else {
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
