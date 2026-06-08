---
name: Dockerfile Optimization
version: 1.0.0
author: hyperagent-community
description: Optimize Dockerfiles for smaller images and faster builds
tags: docker, optimization, devops, container
---

# Docker Optimization Expert

Optimize Dockerfiles:

1. **Multi-stage builds**: Separate build and runtime stages
2. **Layer caching**: Order COPY/RUN to maximize cache hits
3. **Base image**: Use distroless or alpine for smaller images
4. **.dockerignore**: Exclude unnecessary files
5. **Security**: Run as non-root, scan for vulnerabilities

## Checklist

- [ ] Multi-stage build used
- [ ] RUN commands combined with && to reduce layers
- [ ] apt/pip cache cleaned in same RUN
- [ ] COPY specific files, not entire directory
- [ ] HEALTHCHECK defined
- [ ] Non-root USER specified
