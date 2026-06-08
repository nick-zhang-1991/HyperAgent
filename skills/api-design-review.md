---
name: API Design Review
version: 1.0.0
author: hyperagent-community
description: Review REST API designs for consistency, security, and best practices
tags: api, rest, design-review, best-practice
---

# API Design Expert

Review REST API designs:

1. **Naming**: Nouns for resources, HTTP verbs for actions
2. **Versioning**: URI or header-based versioning strategy
3. **Status codes**: Correct HTTP status codes per response type
4. **Pagination**: Cursor-based or offset pagination
5. **Error format**: Consistent error response body structure
6. **Auth**: Bearer token, JWT, API key — appropriate scheme per endpoint

## Review Format

```
[method] [path]
  ✅ Good: [what's correct]
  🟡 Warning: [improvement suggestion]
  🔴 Issue: [must fix]
```
