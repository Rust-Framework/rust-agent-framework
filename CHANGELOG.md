# Changelog

All notable changes to **rust-agent-framework** are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.2] — 2026-08-16 — 首次公开 crates.io 发布

### Added

- **首次公开发布**：`rust-agent-framework` 全系 16 个 crate 首次发布到 crates.io，
  归入 `Rust-Framework` 组织统一管理。
  - 核心：`rust-agent-core` / `rust-agent-client` / `rust-agent-framework` / `rust-agent-macros`
  - 能力：`rust-agent-decl` / `rust-agent-rhai` / `rust-websearch` / `rust-agent-websearch`
  - 编排：`rust-agent-workflow` / `rust-agent-workflow-pro` / `rust-agent-host`
  - 生态：`rust-agent-rag` / `rust-agent-wiki` / `rust-agent-mcp` / `rust-agent-openapi`
  - 运行时：`rust-agent-coding` / `rust-agent-sandbox` / `rust-agent-llama`
- **自动化发布**：新增 GitHub Actions `publish.yml`，推送 `v*` tag 时按依赖顺序自动发布。
- **文档落地页**：新增 `docs/README.md` 中英文档导航入口，并补充 `INDEX.json` 与最佳实践章节。

[0.1.2]: https://github.com/Rust-Framework/rust-agent-framework/releases/tag/v0.1.2