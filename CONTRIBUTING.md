# Contributing to Bridge

Thanks for your interest in contributing to Bridge.

## Getting Started

1. Fork the repository and create a branch for your change.
2. Keep changes focused and include tests when behavior changes.
3. Run checks locally before opening a pull request.

## Local Development

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

## Pull Request Guidelines

- Use clear commit messages.
- Explain the intent and scope of your PR.
- Reference related issues when applicable.
- Update documentation for user-facing or operational changes.

## Code Standards

- Follow idiomatic Rust and existing project patterns.
- Prefer small, reviewable pull requests.
- Avoid introducing breaking changes without prior discussion.

## Community Conduct

Be respectful and constructive in all interactions.
