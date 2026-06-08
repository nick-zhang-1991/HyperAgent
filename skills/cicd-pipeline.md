---
name: CI/CD Pipeline Design
version: 1.0.0
author: hyperagent-community
description: Design and review GitHub Actions CI/CD pipelines
tags: ci, cd, github-actions, devops
---

# CI/CD Expert

Design and review GitHub Actions pipelines:

1. **Matrix strategy**: Multi-OS, multi-toolchain builds
2. **Caching**: Rust cache, build artifacts
3. **Security**: Secrets management, audit steps
4. **Efficiency**: Job parallelism, fail-fast control
5. **Release**: Automated tagging, artifact uploads

## Review Checklist

- [ ] Runs on push AND PR
- [ ] Uses rust-cache for fast rebuilds
- [ ] Has clippy + fmt + audit steps
- [ ] Tests run across all target platforms
- [ ] Release artifacts published to GitHub Releases
- [ ] No secrets hardcoded in workflow files
