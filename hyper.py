#!/usr/bin/env python3
"""
HyperAgent - Ultra-Fast CLI Coding Agent (Python Edition)

A practical, immediately-functional version demonstrating the core architecture:
- TurboIndex: tree-sitter based code indexing with PageRank
- Multi-Agent Pipeline: concurrent specialized agents
- Streaming-First: all LLM calls use streaming

Usage:
  hyper init              # Build code index for current project
  hyper <task>            # Execute a coding task
  hyper --stats           # Show index statistics
  hyper --agents 5 <task> # Custom parallel agent count

Requirements: pip install tree-sitter requests json5 rich
"""

import argparse
import json
import os
import sys
import time
import hashlib
import sqlite3
import re
import subprocess
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass, field
from typing import Optional
from collections import defaultdict

try:
    import requests
except ImportError:
    print("Installing dependencies...")
    subprocess.check_call([sys.executable, "-m", "pip", "install", "requests", "rich", "-q"])
    import requests

try:
    from rich.console import Console
    from rich.panel import Panel
    from rich.syntax import Syntax
    from rich.progress import Progress, SpinnerColumn, TextColumn
    from rich.table import Table
    from rich.markdown import Markdown
    from rich import print as rprint
    console = Console()
except ImportError:
    console = None

# ==========================================
# Configuration
# ==========================================

DEFAULT_MODEL = "deepseek-v4-flash"
DEFAULT_BASE_URL = "https://api.deepseek.com/v1"

# ==========================================
# Symbol & Index Types
# ==========================================

@dataclass
class Symbol:
    name: str
    kind: str  # function, class, struct, enum, trait, interface, method, variable, import, macro
    start_line: int
    end_line: int
    signature: str = ""

@dataclass
class FileSymbols:
    file_path: str
    rel_path: str
    language: str
    symbols: list = field(default_factory=list)

@dataclass
class IndexStats:
    files: int = 0
    symbols: int = 0
    references: int = 0
    languages: int = 0
    cache_size: str = "0B"

# ==========================================
# Code Indexer (TurboIndex)
# ==========================================

IGNORE_DIRS = {
    'node_modules', '.git', 'target', 'build', 'dist', '.hyper',
    '__pycache__', 'venv', '.venv', '.next', '.svelte-kit',
    '.claude', '.hermes', '.cursor', '.vscode', 'idea', '.idea',
    'coverage', '.nyc_output', 'bower_components', 'vendor',
    '.tox', '.eggs', '*.egg-info', '.mypy_cache', '.pytest_cache',
    '.gradle', '.kotlin', '.dart_tool', '*.generated.*'
}

# Language detection by extension
LANG_EXTENSIONS = {
    '.rs': 'rust', '.py': 'python', '.js': 'javascript', '.mjs': 'javascript',
    '.cjs': 'javascript', '.ts': 'typescript', '.tsx': 'typescript',
    '.jsx': 'jsx', '.go': 'go', '.java': 'java', '.c': 'c', '.h': 'c',
    '.cpp': 'cpp', '.cc': 'cpp', '.cxx': 'cpp', '.hpp': 'cpp', '.hh': 'cpp',
    '.rb': 'ruby', '.php': 'php', '.swift': 'swift', '.kt': 'kotlin',
    '.kts': 'kotlin', '.scala': 'scala', '.ex': 'elixir', '.exs': 'elixir',
    '.hs': 'haskell', '.lua': 'lua', '.sh': 'bash', '.bash': 'bash',
    '.zsh': 'bash', '.sql': 'sql', '.r': 'r', '.R': 'r', '.dart': 'dart',
    '.zig': 'zig', '.toml': 'config', '.yaml': 'config', '.yml': 'config',
    '.json': 'config', '.xml': 'config', '.html': 'config', '.css': 'config',
    '.scss': 'config', '.md': 'config', '.vue': 'config',
}

# Regex patterns for symbol extraction (fallback when tree-sitter not available)
SYMBOL_PATTERNS = {
    'rust': [
        (r'(?:pub\s+)?fn\s+(\w+)', 'function'),
        (r'(?:pub\s+)?struct\s+(\w+)', 'struct'),
        (r'(?:pub\s+)?trait\s+(\w+)', 'trait'),
        (r'(?:pub\s+)?enum\s+(\w+)', 'enum'),
        (r'(?:pub\s+)?impl(?:\s*<[^>]*>)?\s+(\w+)', 'implementation'),
        (r'use\s+([\w:]+)', 'import'),
        (r'(?:pub\s+)?mod\s+(\w+)', 'module'),
        (r'(?:pub\s+)?type\s+(\w+)', 'type_alias'),
        (r'(?:pub\s+)?const\s+(\w+)', 'constant'),
        (r'(?:pub\s+)?(?:async\s+)?fn\s+(\w+)', 'function'),
    ],
    'python': [
        (r'^(?:async\s+)?def\s+(\w+)\s*\(', 'function'),
        (r'^class\s+(\w+)', 'class'),
        (r'^import\s+(\S+)', 'import'),
        (r'^from\s+(\S+)\s+import', 'import'),
        (r'^\s*@(\w+)', 'decorator'),
        (r'^\s*(\w+)\s*=\s*(?:lambda|class|def)', 'variable'),
    ],
    'javascript': [
        (r'(?:export\s+)?(?:async\s+)?function\s+(\w+)', 'function'),
        (r'(?:export\s+)?class\s+(\w+)', 'class'),
        (r'(?:export\s+)?const\s+(\w+)\s*=\s*(?:async\s*)?\(', 'arrow_function'),
        (r'(?:export\s+)?const\s+(\w+)\s*=\s*function', 'function'),
        (r'(?:import\s+\{?[^}]+\}?\s+from\s+[\'"])(\S+)(?:[\'"])', 'import'),
        (r'require\([\'"]([^\'"]+)[\'"]\)', 'import'),
        (r'(?:export\s+)?interface\s+(\w+)', 'interface'),
        (r'(?:export\s+)?type\s+(\w+)\s*=', 'type'),
    ],
    'typescript': [
        (r'(?:export\s+)?(?:async\s+)?function\s+(\w+)', 'function'),
        (r'(?:export\s+)?class\s+(\w+)', 'class'),
        (r'(?:export\s+)?(?:abstract\s+)?class\s+(\w+)', 'class'),
        (r'(?:export\s+)?interface\s+(\w+)', 'interface'),
        (r'(?:export\s+)?type\s+(\w+)\s*=', 'type_alias'),
        (r'(?:export\s+)?enum\s+(\w+)', 'enum'),
        (r'(?:export\s+)?const\s+(\w+)\s*=\s*(?:async\s*)?\(', 'arrow_function'),
        (r'(?:import\s+\{?[^}]+\}?\s+from\s+[\'"])(\S+)(?:[\'"])', 'import'),
        (r'(?:export\s+)?default\s+(?:class|function)\s+(\w+)', 'default_export'),
    ],
    'go': [
        (r'^func\s+(\w+)', 'function'),
        (r'^func\s+\([^)]+\)\s+(\w+)', 'method'),
        (r'^type\s+(\w+)\s+struct', 'struct'),
        (r'^type\s+(\w+)\s+interface', 'interface'),
        (r'^import\s+[\'"]([^\'"]+)[\'"]', 'import'),
    ],
    'java': [
        (r'(?:public|private|protected)?\s*(?:static\s+)?(?:class)\s+(\w+)', 'class'),
        (r'(?:public|private|protected)?\s*(?:static\s+)?(?:interface)\s+(\w+)', 'interface'),
        (r'(?:public|private|protected)?\s*(?:static\s+)?\w+\s+(\w+)\s*\(', 'method'),
        (r'import\s+([\w.]+);', 'import'),
    ],
}

class CodeIndexer:
    """HyperIndex: Global code understanding with tree-sitter and PageRank"""

    def __init__(self, root_dir: str):
        self.root_dir = Path(root_dir).resolve()
        self.cache_dir = self.root_dir / ".hyper"
        self.cache_dir.mkdir(parents=True, exist_ok=True)

        self.files = {}  # rel_path -> FileSymbols
        self.graph = defaultdict(set)  # file_idx -> set of (target_idx, weight)
        self.file_list = []  # list of paths
        self.pagerank_scores = []
        self.db_path = self.cache_dir / "index.db"
        self._init_db()

    def _init_db(self):
        """Initialize SQLite cache"""
        os.makedirs(self.cache_dir, exist_ok=True)
        conn = sqlite3.connect(str(self.db_path))
        conn.execute("""
            CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY,
                path TEXT UNIQUE,
                rel_path TEXT,
                language TEXT,
                pagerank REAL DEFAULT 0.0
            )
        """)
        conn.execute("""
            CREATE TABLE IF NOT EXISTS symbols (
                id INTEGER PRIMARY KEY,
                file_id INTEGER,
                name TEXT,
                kind TEXT,
                start_line INTEGER,
                end_line INTEGER,
                signature TEXT,
                FOREIGN KEY (file_id) REFERENCES files(id)
            )
        """)
        conn.execute("""
            CREATE TABLE IF NOT EXISTS ref_edges (
                from_file INTEGER,
                to_file INTEGER,
                weight REAL DEFAULT 1.0,
                PRIMARY KEY (from_file, to_file)
            )
        """)
        conn.execute("CREATE INDEX IF NOT EXISTS idx_sym_name ON symbols(name)")
        conn.execute("CREATE INDEX IF NOT EXISTS idx_sym_file ON symbols(file_id)")
        conn.commit()
        conn.close()

    def has_cache(self) -> bool:
        conn = sqlite3.connect(str(self.db_path))
        count = conn.execute("SELECT COUNT(*) FROM files").fetchone()[0]
        conn.close()
        return count > 0

    def build(self) -> IndexStats:
        """Build full index from scratch"""
        if console:
            console.print("[bold cyan]🔍 Scanning project...[/bold cyan]")

        source_files = self._collect_source_files()
        total = len(source_files)

        if console:
            task = Progress(
                SpinnerColumn(),
                TextColumn("[progress.description]{task.description}"),
                transient=True,
            )
            task.add_task("[yellow]Indexing files...", total=total)
            with Progress(SpinnerColumn(), TextColumn("[progress.description]{task.description}")) as p:
                p_task = p.add_task("[yellow]Parsing source files...", total=total)
                for i, (rel_path, full_path, lang) in enumerate(source_files):
                    self._parse_file(full_path, rel_path, lang)
                    p.update(p_task, advance=1)
        else:
            for i, (rel_path, full_path, lang) in enumerate(source_files):
                self._parse_file(full_path, rel_path, lang)
                if (i + 1) % 20 == 0:
                    print(f"\r   Parsing: {i+1}/{total}", end="")
            print(f"\r   Parsed {total} files")

        # Build reference graph
        self._build_reference_graph()

        # Compute PageRank
        self._compute_pagerank()

        # Save to cache
        self._save_cache()

        stats = IndexStats(
            files=len(self.file_list),
            symbols=sum(len(fs.symbols) for fs in self.files.values()),
            references=sum(len(edges) for edges in self.graph.values()),
            languages=len(set(fs.language for fs in self.files.values())),
        )

        if console:
            console.print(f"[green]✅ Indexed {stats.files} files, {stats.symbols} symbols[/green]")
        return stats

    def _collect_source_files(self):
        """Walk project directory and find source files"""
        files = []
        for root, dirs, names in os.walk(str(self.root_dir)):
            # Filter ignored directories
            dirs[:] = [d for d in dirs if d not in IGNORE_DIRS and not d.startswith('.')]

            for name in names:
                ext = os.path.splitext(name)[1].lower()
                if ext in LANG_EXTENSIONS:
                    full_path = os.path.join(root, name)
                    rel_path = os.path.relpath(full_path, str(self.root_dir))
                    lang = LANG_EXTENSIONS[ext]
                    files.append((rel_path, full_path, lang))

        return files

    def _parse_file(self, full_path: str, rel_path: str, language: str):
        """Parse a file and extract symbols"""
        try:
            with open(full_path, 'r', encoding='utf-8', errors='replace') as f:
                content = f.read()
        except Exception:
            return

        symbols = self._extract_symbols(content, language)

        fs = FileSymbols(
            file_path=full_path,
            rel_path=rel_path,
            language=language,
            symbols=symbols,
        )

        self.files[rel_path] = fs
        idx = len(self.file_list)
        self.file_list.append(rel_path)

    def _extract_symbols(self, content: str, language: str) -> list:
        """Extract symbols using regex patterns"""
        symbols = []
        lines = content.split('\n')

        # Try tree-sitter first (if available)
        symbols = self._try_treesitter(content, language)

        # Fallback to regex
        if not symbols:
            patterns = SYMBOL_PATTERNS.get(language, [])
            for i, line in enumerate(lines):
                for pattern, kind in patterns:
                    match = re.search(pattern, line)
                    if match:
                        try:
                            name = match.group(1).strip()
                        except IndexError:
                            continue
                        if len(name) <= 100 and not name.startswith(('#', '//', '/*', '*')):
                            symbols.append(Symbol(
                                name=name,
                                kind=kind,
                                start_line=i + 1,
                                end_line=i + 1,
                                signature=line.strip()[:120],
                            ))
                            break

        return symbols

    def _try_treesitter(self, content: str, language: str) -> list:
        """Try to use tree-sitter for parsing"""
        symbols = []
        try:
            import tree_sitter_rust as ts_rust
            import tree_sitter_python as ts_python
            import tree_sitter_javascript as ts_javascript
            from tree_sitter import Language, Parser

            lang_map = {
                'rust': ts_rust.language(),
                'python': ts_python.language(),
                'javascript': ts_javascript.language(),
            }

            if language not in lang_map:
                return []

            parser = Parser(lang_map[language])
            tree = parser.parse(bytes(content, 'utf-8'))
            root = tree.root_node

            # Walk the tree and extract symbols
            def _walk(node):
                kind = node.type
                if kind in ('function_item', 'function_definition'):
                    name_node = node.child_by_field_name('name')
                    if name_node:
                        symbols.append(Symbol(
                            name=content[name_node.start_byte:name_node.end_byte],
                            kind='function',
                            start_line=node.start_point[0] + 1,
                            end_line=node.end_point[0] + 1,
                            signature=content[node.start_byte:node.end_byte].split('\n')[0][:120],
                        ))

                elif kind in ('struct_item', 'enum_item', 'trait_item'):
                    name_node = node.child_by_field_name('name')
                    if name_node:
                        symbols.append(Symbol(
                            name=content[name_node.start_byte:name_node.end_byte],
                            kind=kind.replace('_item', ''),
                            start_line=node.start_point[0] + 1,
                            end_line=node.end_point[0] + 1,
                            signature=content[node.start_byte:node.end_byte].split('\n')[0][:120],
                        ))

                elif kind == 'class_definition' or kind == 'class_declaration':
                    name_node = node.child_by_field_name('name')
                    if name_node:
                        symbols.append(Symbol(
                            name=content[name_node.start_byte:name_node.end_byte],
                            kind='class',
                            start_line=node.start_point[0] + 1,
                            end_line=node.end_point[0] + 1,
                            signature=content[node.start_byte:node.end_byte].split('\n')[0][:120],
                        ))

                # Recurse
                for child in node.children:
                    _walk(child)

            _walk(root)

        except ImportError:
            pass

        return symbols

    def _build_reference_graph(self):
        """Build file reference graph using import/symbol relationships"""
        file_by_name = {}
        for rel_path, fs in self.files.items():
            file_by_name[rel_path] = len(file_by_name)

        for rel_path, fs in self.files.items():
            for sym in fs.symbols:
                if sym.kind == 'import':
                    # Try to resolve import to a file
                    import_path = sym.name.replace('.', '/').replace('::', '/').split(':')[0]
                    for other_rel, other_fs in self.files.items():
                        if import_path in other_rel or other_rel.replace('.py', '').replace('.rs', '') == import_path:
                            i = file_by_name.get(rel_path)
                            j = file_by_name.get(other_rel)
                            if i is not None and j is not None and i != j:
                                self.graph[i].add((j, 1.0))

            # Connect files in same directory (likely related)
            dir_name = os.path.dirname(rel_path)
            for other_rel in self.files:
                if other_rel != rel_path and os.path.dirname(other_rel) == dir_name:
                    i = file_by_name.get(rel_path)
                    j = file_by_name.get(other_rel)
                    if i is not None and j is not None and i != j:
                        self.graph[i].add((j, 0.5))

    def _compute_pagerank(self):
        """Compute PageRank scores for all files"""
        n = len(self.file_list)
        if n == 0:
            return

        damping = 0.85
        max_iter = 100
        tol = 1e-6

        scores = [1.0 / n] * n
        new_scores = [0.0] * n

        out_degree = []
        for i in range(n):
            out_degree.append(len(self.graph.get(i, set())))

        for _ in range(max_iter):
            total_diff = 0.0
            for i in range(n):
                s = 0.0
                for j in range(n):
                    if i != j and out_degree[j] > 0:
                        edges = self.graph.get(j, set())
                        for (target, weight) in edges:
                            if target == i:
                                s += scores[j] * weight / out_degree[j]

                new_scores[i] = (1.0 - damping) / n + damping * s
                total_diff += abs(new_scores[i] - scores[i])

            scores, new_scores = new_scores, scores
            if total_diff < tol:
                break

        self.pagerank_scores = scores

    def _save_cache(self):
        """Save index to SQLite cache"""
        conn = sqlite3.connect(str(self.db_path))
        conn.execute("DELETE FROM files")
        conn.execute("DELETE FROM symbols")
        conn.execute("DELETE FROM ref_edges")

        for i, rel_path in enumerate(self.file_list):
            fs = self.files.get(rel_path)
            if not fs:
                continue
            score = self.pagerank_scores[i] if i < len(self.pagerank_scores) else 0.0
            conn.execute(
                "INSERT INTO files (id, path, rel_path, language, pagerank) VALUES (?, ?, ?, ?, ?)",
                (i, fs.file_path, rel_path, fs.language, score)
            )
            for sym in fs.symbols:
                conn.execute(
                    "INSERT INTO symbols (file_id, name, kind, start_line, end_line, signature) VALUES (?, ?, ?, ?, ?, ?)",
                    (i, sym.name, sym.kind, sym.start_line, sym.end_line, sym.signature)
                )

        for i, edges in self.graph.items():
            for j, w in edges:
                conn.execute(
                    "INSERT OR REPLACE INTO ref_edges (from_file, to_file, weight) VALUES (?, ?, ?)",
                    (i, j, w)
                )

        conn.commit()
        conn.close()

    def _load_cache(self):
        """Load index from SQLite cache"""
        conn = sqlite3.connect(str(self.db_path))

        rows = conn.execute("SELECT id, path, rel_path, language, pagerank FROM files ORDER BY id").fetchall()
        for row in rows:
            idx, path, rel_path, language, pagerank = row
            self.file_list.append(rel_path)
            self.files[rel_path] = FileSymbols(
                file_path=path,
                rel_path=rel_path,
                language=language,
                symbols=[],
            )

        for row in conn.execute("SELECT file_id, name, kind, start_line, end_line, signature FROM symbols"):
            file_id, name, kind, start_line, end_line, signature = row
            rel_path = self.file_list[file_id] if file_id < len(self.file_list) else None
            if rel_path and rel_path in self.files:
                self.files[rel_path].symbols.append(Symbol(
                    name=name, kind=kind,
                    start_line=start_line, end_line=end_line, signature=signature
                ))

        for row in conn.execute("SELECT from_file, to_file, weight FROM ref_edges"):
            from_f, to_f, weight = row
            self.graph[from_f].add((to_f, weight))

        self.pagerank_scores = [0.0] * len(self.file_list)
        for row in conn.execute("SELECT id, pagerank FROM files"):
            idx, score = row
            if idx < len(self.pagerank_scores):
                self.pagerank_scores[idx] = score

        conn.close()

    def get_relevant_files(self, query: str, max_files: int = 15) -> list:
        """Get the most relevant files for a given query"""
        query_lower = query.lower()
        query_words = query_lower.split()

        if not query_words:
            return []

        # Score files: PageRank + query match
        scored = []
        for i, rel_path in enumerate(self.file_list):
            score = self.pagerank_scores[i] if i < len(self.pagerank_scores) else 0.0

            # Boost by symbol match
            fs = self.files.get(rel_path)
            if fs:
                for sym in fs.symbols:
                    name_lower = sym.name.lower()
                    match_count = sum(1 for w in query_words if w in name_lower)
                    score += match_count * 0.2

                # Boost by path match
                path_lower = rel_path.lower()
                match_count = sum(1 for w in query_words if w in path_lower)
                score += match_count * 0.1

            scored.append((score, rel_path))

        scored.sort(reverse=True, key=lambda x: x[0])
        top_files = scored[:max_files]

        result = []
        for score, rel_path in top_files:
            fs = self.files.get(rel_path)
            if fs:
                try:
                    with open(fs.file_path, 'r', encoding='utf-8', errors='replace') as f:
                        content = f.read()
                    result.append({
                        'path': fs.file_path,
                        'rel_path': rel_path,
                        'score': score,
                        'content': content,
                        'total_lines': len(content.split('\n')),
                        'language': fs.language,
                    })
                except Exception:
                    pass

        return result

    def get_stats(self) -> IndexStats:
        """Get index statistics"""
        total_symbols = sum(len(fs.symbols) for fs in self.files.values())
        total_refs = sum(len(edges) for edges in self.graph.values())
        languages = len(set(fs.language for fs in self.files.values()))

        try:
            size = os.path.getsize(str(self.db_path))
            if size < 1024:
                size_str = f"{size}B"
            elif size < 1024 * 1024:
                size_str = f"{size/1024:.1f}KB"
            else:
                size_str = f"{size/(1024*1024):.1f}MB"
        except:
            size_str = "0B"

        return IndexStats(
            files=len(self.file_list),
            symbols=total_symbols,
            references=total_refs,
            languages=languages,
            cache_size=size_str,
        )


# ==========================================
# LLM Provider
# ==========================================

class LlmProvider:
    """OpenAI-compatible LLM provider with streaming"""

    def __init__(self, model: str = None, base_url: str = None, api_key: str = None):
        self.model = model or os.environ.get('HYPER_MODEL') or os.environ.get('DEEPSEEK_MODEL') or DEFAULT_MODEL
        self.base_url = (base_url or os.environ.get('HYPER_LLM_BASE_URL')
                         or os.environ.get('DEEPSEEK_BASE_URL') or DEFAULT_BASE_URL).rstrip('/')
        self.api_key = (api_key or os.environ.get('HYPER_LLM_API_KEY')
                        or os.environ.get('DEEPSEEK_API_KEY') or '')

        if not self.api_key:
            print("⚠️  No API key found. Set HYPER_LLM_API_KEY or DEEPSEEK_API_KEY")
            self.api_key = ""

    def chat(self, messages: list, stream: bool = False) -> str:
        """Send a chat completion request"""
        url = f"{self.base_url}/chat/completions"
        headers = {
            "Authorization": f"Bearer {self.api_key}",
            "Content-Type": "application/json"
        }
        body = {
            "model": self.model,
            "messages": messages,
            "stream": stream,
            "temperature": 0.1,
            "max_tokens": 16384,
        }

        resp = requests.post(url, headers=headers, json=body, timeout=180, stream=stream)

        if resp.status_code != 200:
            raise Exception(f"API error {resp.status_code}: {resp.text[:500]}")

        if stream:
            full_response = []
            for line in resp.iter_lines():
                if line:
                    line = line.decode('utf-8', errors='replace')
                    if line.startswith('data: '):
                        data = line[6:]
                        if data == '[DONE]':
                            break
                        try:
                            chunk = json.loads(data)
                            content = chunk.get('choices', [{}])[0].get('delta', {}).get('content', '')
                            if content:
                                full_response.append(content)
                        except json.JSONDecodeError:
                            pass
            return ''.join(full_response)
        else:
            data = resp.json()
            return data['choices'][0]['message']['content']


# ==========================================
# Multi-Agent Pipeline
# ==========================================

class PlanAgent:
    """Analyzes task and creates execution plan"""

    def __init__(self, llm: LlmProvider):
        self.llm = llm

    def create_plan(self, prompt: str, files: list) -> dict:
        system = """You are HyperAgent's PlanAgent. Analyze the coding task and create a plan.

Given the task and relevant code files, output a JSON plan:
{
  "summary": "Brief description of what needs to be done",
  "steps": ["Step 1: ...", "Step 2: ...", ...],
  "affected_files": ["path/to/file1", ...]
}

Be specific, order logically, and make each step independently executable."""

        file_context = self._build_context(files)
        user_msg = f"Task: {prompt}\n\nRelevant files:\n{file_context}\n\nOutput the JSON plan."

        response = self.llm.chat([
            {"role": "system", "content": system},
            {"role": "user", "content": user_msg},
        ])

        return self._parse_plan(response)

    def _build_context(self, files: list) -> str:
        ctx = ""
        for i, f in enumerate(files[:10]):
            ctx += f"[{i+1}] {f['rel_path']} ({f['total_lines']} lines, score: {f['score']:.3f})\n"
            preview = '\n'.join(f['content'].split('\n')[:15])
            ctx += f"  ```\n{preview}\n  ```\n\n"
        return ctx

    def _parse_plan(self, response: str) -> dict:
        try:
            start = response.index('{')
            end = response.rindex('}')
            plan = json.loads(response[start:end+1])
            return plan
        except (ValueError, json.JSONDecodeError):
            return {
                "summary": response[:200],
                "steps": [response],
                "affected_files": []
            }


class CodeAgent:
    """Executes code changes for specific plan steps"""

    def __init__(self, llm: LlmProvider, root_dir: str, agent_id: int = 0):
        self.llm = llm
        self.root_dir = root_dir
        self.agent_id = agent_id

    def execute(self, prompt: str, steps: list, files: list) -> list:
        if not steps:
            return []

        system = """You are HyperAgent's CodeAgent. You write precise, production-quality code.

For each step, output JSON changes in this format:
{"file": "relative/path/to/file", "change_type": "edit|create|delete", "content": "COMPLETE NEW FILE CONTENT"}

Rules:
- Output COMPLETE file content for each change
- Include all imports and dependencies
- Follow existing code style
- Write clean, well-documented code
- Output one JSON object per change, separated by newlines"""

        file_context = ""
        for f in files[:8]:
            lines = f['content'].split('\n')
            if f['total_lines'] > 150:
                preview = '\n'.join(lines[:60]) + "\n... (skipped) ...\n" + '\n'.join(lines[-30:])
            else:
                preview = f['content']
            file_context += f"--- {f['rel_path']} ---\n{preview}\n\n"

        user_msg = f"Task: {prompt}\n\nSteps to execute:\n{chr(10).join('- ' + s for s in steps)}\n\nCurrent files:\n{file_context}\n\nOutput JSON changes."

        response = self.llm.chat([
            {"role": "system", "content": system},
            {"role": "user", "content": user_msg},
        ], stream=True)

        return self._parse_changes(response)

    def _parse_changes(self, response: str) -> list:
        changes = []
        for match in re.finditer(r'\{[^}]+\}', response):
            try:
                change = json.loads(match.group())
                if 'file' in change and 'change_type' in change:
                    file_path = os.path.join(self.root_dir, change['file'])
                    old_content = None
                    if os.path.exists(file_path):
                        with open(file_path, 'r') as f:
                            old_content = f.read()
                    changes.append({
                        'file': file_path,
                        'rel_path': change['file'],
                        'change_type': change.get('change_type', 'edit'),
                        'old_content': old_content,
                        'new_content': change.get('content', ''),
                        'agent_id': self.agent_id,
                    })
            except json.JSONDecodeError:
                continue
        return changes


class ReviewAgent:
    """Validates changes before application"""

    def __init__(self, llm: LlmProvider):
        self.llm = llm

    def review(self, task: str, changes: list) -> list:
        if not changes:
            return changes

        review_input = f"Task: {task}\n\nProposed changes:\n"
        for i, ch in enumerate(changes):
            review_input += f"\n--- Change {i}: {ch.get('rel_path', ch['file'])} ({ch['change_type']}) ---\n"
            if ch.get('old_content'):
                review_input += f"OLD:\n```\n{ch['old_content'][:500]}\n```\n"
            if ch.get('new_content'):
                review_input += f"NEW:\n```\n{ch['new_content'][:500]}\n```\n"

        system = """You are HyperAgent's ReviewAgent. Validate code changes.

Output JSON: {"approved": [0, 2], "rejected": [1], "reasons": {"1": "Missing import"}}

APROVE only if: solves the problem, maintains correctness, no security issues, follows style."""

        response = self.llm.chat([
            {"role": "system", "content": system},
            {"role": "user", "content": review_input},
        ])

        try:
            start = response.index('{')
            end = response.rindex('}')
            result = json.loads(response[start:end+1])
            approved = result.get('approved', list(range(len(changes))))
            return [changes[i] for i in approved if i < len(changes)]
        except (ValueError, json.JSONDecodeError, TypeError):
            return changes  # Approve all on parse error


class ApplyAgent:
    """Applies changes to filesystem"""

    def __init__(self, root_dir: str, confirm: bool = True):
        self.root_dir = root_dir
        self.confirm = confirm

    def apply(self, changes: list) -> list:
        messages = []
        for change in changes:
            file_path = change['file']
            change_type = change['change_type']
            content = change.get('new_content', '')

            if self.confirm:
                if console:
                    console.print(f"\n[bold]📝 {file_path}[/bold] [dim]({change_type})[/dim]")
                    if change.get('old_content') and content:
                        from difflib import unified_diff
                        diff = list(unified_diff(
                            change['old_content'].splitlines(True),
                            content.splitlines(True),
                            fromfile='a/' + change.get('rel_path', ''),
                            tofile='b/' + change.get('rel_path', ''),
                        ))
                        syntax = Syntax(''.join(diff[:40]), "diff")
                        console.print(syntax)
                    else:
                        console.print(Syntax(content[:500], "python" if content else "text"))
                else:
                    print(f"\n📝 {file_path} ({change_type})")

                if input("   Apply? [Y/n/q]: ").lower() in ('n', 'no'):
                    messages.append(f"Skipped: {file_path}")
                    continue
                elif input("   Apply? [Y/n/q]: ").lower() in ('q', 'quit'):
                    messages.append("Cancelled by user")
                    break

            os.makedirs(os.path.dirname(file_path), exist_ok=True)
            with open(file_path, 'w') as f:
                f.write(content)
            messages.append(f"✓ {change_type}: {file_path}")

        return messages


class Orchestrator:
    """Main orchestrator with multi-agent parallelism"""

    def __init__(self, indexer: CodeIndexer, llm: LlmProvider, root_dir: str,
                 parallel_agents: int = 3, confirm: bool = True):
        self.indexer = indexer
        self.llm = llm
        self.root_dir = root_dir
        self.parallel_agents = parallel_agents
        self.confirm = confirm

    def run(self, prompt: str) -> dict:
        start_time = time.time()

        if console:
            console.print(f"\n[bold cyan]🚀 HyperAgent[/bold cyan] - Processing: [italic]{prompt}[/italic]\n")
        else:
            print(f"\n🚀 HyperAgent - Processing: {prompt}\n")

        # Phase 1: Get relevant files
        if console:
            with console.status("[bold yellow]🔍 Analyzing codebase...") as status:
                relevant_files = self.indexer.get_relevant_files(prompt, 15)
                status.update(f"[green]Found {len(relevant_files)} relevant files[/green]")
        else:
            print("🔍 Analyzing codebase...")
            relevant_files = self.indexer.get_relevant_files(prompt, 15)
            print(f"   Found {len(relevant_files)} relevant files")

        # Phase 2: Planning
        if console:
            with console.status("[bold yellow]📋 Planning...") as status:
                plan_agent = PlanAgent(self.llm)
                plan = plan_agent.create_plan(prompt, relevant_files)
        else:
            print("📋 Planning...")
            plan_agent = PlanAgent(self.llm)
            plan = plan_agent.create_plan(prompt, relevant_files)

        if console:
            console.print(f"[bold]Plan:[/bold] {plan.get('summary', '')}")
            for i, step in enumerate(plan.get('steps', [])):
                console.print(f"  {i+1}. {step}")
        else:
            print(f"Plan: {plan.get('summary', '')}")
            for i, step in enumerate(plan.get('steps', [])):
                print(f"  {i+1}. {step}")

        plan_start = time.time()
        plan_time = plan_start - start_time

        # Phase 3: Parallel code execution
        if console:
            console.print(f"\n[bold]👨‍💻 Executing with {self.parallel_agents} parallel agents...[/bold]")
        else:
            print(f"\n👨‍💻 Executing with {self.parallel_agents} parallel agents...")

        steps = plan.get('steps', [])
        if not steps:
            steps = ["Execute the task as described"]

        # Split steps among agents
        chunks = [[] for _ in range(self.parallel_agents)]
        for i, step in enumerate(steps):
            chunks[i % self.parallel_agents].append(step)

        all_changes = []
        code_start = time.time()

        with ThreadPoolExecutor(max_workers=self.parallel_agents) as executor:
            futures = []
            for agent_id, chunk in enumerate(chunks):
                if chunk:
                    agent = CodeAgent(self.llm, self.root_dir, agent_id)
                    future = executor.submit(agent.execute, prompt, chunk, relevant_files)
                    futures.append(future)

            for future in as_completed(futures):
                try:
                    changes = future.result(timeout=180)
                    all_changes.extend(changes)
                    if console:
                        console.print(f"   [dim]Agent produced {len(changes)} changes[/dim]")
                except Exception as e:
                    if console:
                        console.print(f"   [red]Agent error: {e}[/red]")
                    else:
                        print(f"   Agent error: {e}")

        code_time = time.time() - code_start

        if not all_changes:
            if console:
                console.print("[yellow]⚠️ No changes produced[/yellow]")
            else:
                print("⚠️ No changes produced")
            return {"files_modified": 0, "tokens_used": 0, "time": time.time() - start_time}

        if console:
            console.print(f"[green]Total: {len(all_changes)} changes from {len(futures)} agents[/green]")
        else:
            print(f"Total: {len(all_changes)} changes")

        # Phase 4: Review
        if console:
            with console.status("[bold yellow]🔎 Reviewing changes...") as status:
                review_agent = ReviewAgent(self.llm)
                approved = review_agent.review(prompt, all_changes)
                status.update(f"[green]Approved {len(approved)}/{len(all_changes)} changes[/green]")
        else:
            print("🔎 Reviewing changes...")
            review_agent = ReviewAgent(self.llm)
            approved = review_agent.review(prompt, all_changes)
            print(f"Approved {len(approved)}/{len(all_changes)} changes")

        if not approved:
            print("⚠️ All changes rejected during review")
            return {"files_modified": 0, "tokens_used": 0, "time": time.time() - start_time}

        # Phase 5: Apply
        if console:
            console.print(f"\n[bold]✏️ Applying {len(approved)} changes...[/bold]")
        else:
            print(f"\n✏️ Applying {len(approved)} changes...")

        apply_agent = ApplyAgent(self.root_dir, self.confirm)
        results = apply_agent.apply(approved)

        for msg in results:
            if console:
                console.print(f"   {msg}")
            else:
                print(f"   {msg}")

        total_time = time.time() - start_time
        if console:
            console.print(f"\n[bold green]✅ Complete[/bold green] [dim]in {total_time:.1f}s (plan: {plan_time:.1f}s, code: {code_time:.1f}s)[/dim]")
        else:
            print(f"\n✅ Complete in {total_time:.1f}s")

        return {
            "files_modified": len(approved),
            "tokens_used": sum(len(c.get('new_content', '')) for c in approved) // 4,
            "time": total_time,
        }


# ==========================================
# CLI Entry Point
# ==========================================

def build_index_cmd(args):
    """Build the code index"""
    root = Path(args.dir).resolve()
    indexer = CodeIndexer(str(root))

    if indexer.has_cache() and not args.reindex:
        if console:
            console.print("[yellow]📦 Loading cached index...[/yellow]")
        indexer._load_cache()
        stats = indexer.get_stats()
        if console:
            console.print(f"[green]✅ Loaded {stats.files} files, {stats.symbols} symbols from cache[/green]")
        return

    stats = indexer.build()

    if console:
        table = Table(title="HyperIndex Statistics")
        table.add_column("Metric", style="cyan")
        table.add_column("Value", style="green")
        table.add_row("Files", str(stats.files))
        table.add_row("Symbols", str(stats.symbols))
        table.add_row("References", str(stats.references))
        table.add_row("Languages", str(stats.languages))
        table.add_row("Cache", stats.cache_size)
        console.print(table)
    else:
        print(f"\n📊 HyperIndex Statistics")
        print(f"   Files:      {stats.files}")
        print(f"   Symbols:    {stats.symbols}")
        print(f"   References: {stats.references}")
        print(f"   Languages:  {stats.languages}")
        print(f"   Cache:      {stats.cache_size}")


def run_agent_cmd(args):
    """Run the agent with a prompt"""
    root = Path(args.dir).resolve()
    indexer = CodeIndexer(str(root))

    if not indexer.has_cache() or args.reindex:
        if console:
            console.print("[yellow]🔨 Building index first...[/yellow]")
        else:
            print("🔨 Building index first...")
        indexer.build()
    else:
        if console:
            with console.status("[yellow]📦 Loading index...[/yellow]"):
                indexer._load_cache()
        else:
            print("📦 Loading index...")
            indexer._load_cache()

    llm = LlmProvider(args.model)
    orchestrator = Orchestrator(
        indexer, llm, str(root),
        parallel_agents=args.agents,
        confirm=not args.yes,
    )
    result = orchestrator.run(args.prompt)
    return result


def main():
    parser = argparse.ArgumentParser(
        description="HyperAgent - Ultra-Fast CLI Coding Agent",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  hyper init                  # Build code index
  hyper "add error handling"  # Run agent
  hyper --stats               # Show index stats
  hyper --agents 5 <task>     # 5 parallel agents
  hyper -y "fix the bug"      # Auto-apply changes
        """
    )
    parser.add_argument("prompt", nargs="?", help="Task description")
    parser.add_argument("--dir", "-d", default=".", help="Project directory")
    parser.add_argument("--agents", "-a", type=int, default=3, help="Parallel agents (default: 3)")
    parser.add_argument("--model", "-m", help="Model to use")
    parser.add_argument("--yes", "-y", action="store_true", help="Skip confirmation")
    parser.add_argument("--init", action="store_true", help="Build index")
    parser.add_argument("--reindex", action="store_true", help="Rebuild index from scratch")
    parser.add_argument("--stats", action="store_true", help="Show index statistics")
    parser.add_argument("--debug", action="store_true", help="Enable debug output")

    args = parser.parse_args()

    if args.debug:
        import logging
        logging.basicConfig(level=logging.DEBUG)

    if args.stats:
        root = Path(args.dir).resolve()
        indexer = CodeIndexer(str(root))
        if indexer.has_cache():
            indexer._load_cache()
            if console:
                stats = indexer.get_stats()
                table = Table(title="HyperIndex Statistics")
                table.add_column("Metric", style="cyan")
                table.add_column("Value", style="green")
                table.add_row("Files", str(stats.files))
                table.add_row("Symbols", str(stats.symbols))
                table.add_row("References", str(stats.references))
                table.add_row("Languages", str(stats.languages))
                table.add_row("Cache", stats.cache_size)
                console.print(table)
            else:
                print(f"\n📊 HyperIndex Statistics")
        else:
            print("⚠️  No index found. Run 'hyper --init' first.")
        return

    if args.init or args.reindex:
        build_index_cmd(args)
        return

    if not args.prompt:
        parser.print_help()
        return

    run_agent_cmd(args)


if __name__ == "__main__":
    main()
