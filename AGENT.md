# Agent Guidelines

## Communication

- 使用**中文**与用户交流。

## Project Overview

This is a Rust async application template. New projects should be based on this structure.

## Tech Stack & Conventions

- **Async runtime**: `tokio` (full features)
- **Logging**: Use `log` crate macros (`log::info!`, `log::error!`, etc.). Logger initialization via `custom_utils::logger::logger_feature`. Do NOT use `println!` for application output — always use `log` macros.
- **HTTP client**: `reqwest` with `rustls-tls` backend (no OpenSSL). Always use `reqwest::Client` (async), avoid blocking API.
- **Error handling**: `anyhow::Result` for application-level errors. Use `?` operator for propagation. Add `.context("...")` for meaningful error messages when appropriate.
- **Serialization**: `serde` + `serde_json`.

## Code Structure

- Prefer putting logic in `src/lib.rs` (and sub-modules under `src/`). Keep `src/main.rs` minimal — only runtime bootstrap and logger init.
- When the project grows, split into modules under `src/` and re-export from `lib.rs`.

## Style

- Follow `rustfmt.toml` config: max width 120, 4-space indent.
- Follow `clippy.toml` thresholds.
- Run `cargo fmt --check` to verify formatting before committing.

## Code Quality

- Use `cargo clippy -- -D warnings` to check code. All clippy warnings must be resolved before committing.
- Do NOT suppress clippy warnings with `#[allow(...)]` unless there is a justified reason documented in a comment.
- Prefer `cargo check` for fast compilation verification during development.

## Dependencies

- Do NOT add new dependencies without explicit user approval. Evaluate necessity before introducing a new crate.
- Prefer crates that are well-maintained and have minimal transitive dependencies.
- `Cargo.lock` is gitignored (library-style template). If the project is a deployed binary, consider tracking it.

## Error Handling

- Never use `.unwrap()` or `.expect()` in library code (`src/lib.rs` and modules). Use `?` with `anyhow::Result` instead.
- `.unwrap()` is acceptable only in `main.rs` bootstrap or tests.

## Build & Release

- CI builds two targets: `x86_64-pc-windows-msvc` and `aarch64-unknown-linux-gnu`.
- Release is triggered by pushing a `v*` tag.
- Before pushing a tag, verify locally: `cargo clippy -- -D warnings && cargo fmt --check && cargo test`.
