# Agent Guidelines

## Communication

- 使用**中文**与用户交流。

## Project Overview

Rust 异步应用程序。具体功能描述请补充到这里。

## Tech Stack & Conventions

- **Async runtime**: `tokio` (full features)。
- **Logging**: 使用 `log` crate 的宏（`log::info!`、`log::error!` 等）。日志初始化通过 `custom_utils::logger::logger_feature`。禁止使用 `println!` 输出应用日志。
- **HTTP client**: `reqwest`，使用 `rustls-tls` 后端（不依赖 OpenSSL）。始终使用异步 `reqwest::Client`，禁止使用阻塞 API。
- **Error handling**: 应用层错误使用 `anyhow::Result`，通过 `?` 传播，必要时添加 `.context("...")`。
- **Serialization**: `serde` + `serde_json`。

## Code Structure

- 业务逻辑优先放在 `src/lib.rs` 及 `src/` 下的子模块中。`src/main.rs` 保持精简，只负责运行时启动和日志初始化。
- 项目规模增长时，拆分为 `src/` 下的子模块，通过 `lib.rs` 统一导出。

## Style

- 遵循 `rustfmt.toml` 配置：最大行宽 120，4 空格缩进。
- 遵循 `clippy.toml` 阈值配置。
- 提交前执行 `cargo fmt --check` 验证格式。

## Code Quality

- 使用 `cargo clippy -- -D warnings` 检查代码，所有 clippy 警告必须在提交前解决。
- 禁止使用 `#[allow(...)]` 压制警告，除非有充分理由并在注释中说明。
- 开发过程中优先使用 `cargo check` 快速验证编译。

## Dependencies

- 未经用户明确同意，不得添加新依赖。引入新 crate 前需评估必要性。
- 优先选择维护良好、传递依赖少的 crate。

## Error Handling

- 库代码（`src/lib.rs` 及子模块）中禁止使用 `.unwrap()` 或 `.expect()`，使用 `?` 配合 `anyhow::Result`。
- `.unwrap()` 仅允许在 `main.rs` 启动逻辑和测试中使用。

## Build & Release

- CI 构建两个目标：`x86_64-pc-windows-msvc` 和 `aarch64-unknown-linux-gnu`。
- 通过推送 `v*` 标签触发 Release。
- 推送标签前本地验证：`cargo clippy -- -D warnings && cargo fmt --check && cargo test`。
