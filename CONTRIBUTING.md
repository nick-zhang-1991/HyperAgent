# Contributing to HyperAgent

Thanks for your interest! HyperAgent is an open-source, community-driven project.

## Quick Start

```bash
git clone https://github.com/nick-zhang-1991/HyperAgent.git
cd HyperAgent
cargo build
./target/debug/hyperagent --version
```

## Development

```bash
# Run tests
cargo test --all-targets

# Check formatting
cargo fmt --all --check

# Lint
cargo clippy --all-targets -- -D warnings

# Build release
cargo build --release
```

## Pull Requests

1. Fork the repo
2. Create a feature branch
3. Make changes + add tests
4. Run `cargo test` and `cargo fmt`
5. Submit PR with description

## Adding a Skill

Share your expertise as a community skill:

```bash
hyper skill create "My Skill Name"
# Edit the generated .md file
# Upload to GitHub Gist
hyper skill install <gist-url>
```

See [skills/](skills/) for examples.

## Code of Conduct

Be respectful. We're building together.
