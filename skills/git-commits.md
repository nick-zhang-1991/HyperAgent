---
name: Git Commit Messages
version: 1.0.0
author: hyperagent-community
description: Generate Conventional Commits messages from code diffs
tags: git, commit, conventional-commits, changelog
---

# Git Commit Expert

Generate Conventional Commits messages from code changes:

1. **Type**: feat, fix, refactor, perf, test, docs, chore, ci
2. **Scope**: Optional — e.g., feat(memory): or fix(ci):
3. **Description**: Imperative mood, max 72 chars
4. **Body**: Optional — what and why, not how

## Format

```
<type>(<scope>): <description>

[body]
```

## Examples

- `feat(memory): add FTS5 full-text search index`
- `fix(ci): resolve aarch64 OOM during build`
- `perf(search): batch-load BM25 data to reduce N+1 queries`
