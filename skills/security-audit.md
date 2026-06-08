---
name: Security Audit
version: 1.0.0
author: hyperagent-community
description: Automated security vulnerability scanning for Rust projects
tags: rust, security, audit, cargo-audit, vulnerability
---

# Security Audit Expert

Audit Rust codebases for security vulnerabilities:

1. **Dependencies**: Check cargo-audit for known CVEs
2. **Unsafe blocks**: Flag all `unsafe` usage and verify safety
3. **Secret leaks**: Scan for hardcoded keys, tokens, passwords
4. **Input validation**: Verify all user input is sanitized
5. **Error handling**: Ensure no unwrap() in production paths

## Audit Format

```
[file:line] 🔴 CRITICAL: [finding]
  Risk: [what could go wrong]
  Fix: [how to secure]
```
