# Contributing to Strangecoin

Thank you for considering contributing to Strangecoin! This document outlines the process and standards for contributions.

## Architectural Decision Records (ADRs)

All significant architectural decisions are documented as **Architectural Decision Records (ADRs)** in `docs/ADR/`.

- **Before proposing a change** that affects architecture, consensus, cryptography, or security: check existing ADRs.
- **New decisions** must be documented as an ADR using the template at `docs/ADR/0001-template.md`.
- ADRs are immutable once accepted — they are only superseded by new ADRs.

See `docs/ADR/0001-template.md` for the required structure: Context, Decision, Consequences, Alternatives.

## Development Workflow

1. **Fork** the repository and create a feature branch.
2. **Write code** following the style guide below.
3. **Run checks** before pushing:
   ```bash
   cargo check
   cargo test
   cargo clippy -- -D warnings
   ```
4. **Open a Pull Request** with a clear description of the change and motivation.
5. **Reference ADRs** if your change relates to a documented decision.

## Code Style

- **Rust edition 2021**. Run `cargo fmt` before committing.
- **No comments** unless explaining *why* (not *what*). Code should be self-documenting.
- **Error handling**: Use `thiserror` for error types, `anyhow` for application-level errors.
- **Logging**: Use `tracing` macros (`info!`, `warn!`, `error!`, `debug!`) with structured fields. No `println!` or `eprintln!`.
- **Dependencies**: Minimize external crates. Prefer stdlib. Audit new dependencies for maintenance status and license compatibility (MIT/Apache-2.0).

## Testing

- **Unit tests** in `#[cfg(test)]` modules alongside code.
- **Integration tests** in `tests/` directory.
- **Property-based tests** using `proptest` for consensus-critical logic (serialization, validation, emission).
- All tests must pass in CI before merge.

## Security

- **No secrets in code or config**. Passwords, keys, tokens — only via environment variables or encrypted keystore.
- **Report vulnerabilities** privately via GitHub Security Advisories.
- See `docs/ADR/0004-license.md` and `docs/ADR/0005-tracing-vs-println.md` for licensing and logging decisions.

## Branching & Releases

- `main` — Protected branch. Requires PR review + passing CI.
- Versioning follows SemVer. Stage 0 = `0.x.y` (pre-1.0).
- Tags: `v0.8.6`, `v0.9.0`, etc.

## License

By contributing, you agree that your contributions will be licensed under the dual **MIT OR Apache-2.0** license (see `LICENSE` and `docs/ADR/0004-license.md`).

## Questions?

Open a GitHub Discussion or issue. For security-sensitive topics, use GitHub Security Advisories.