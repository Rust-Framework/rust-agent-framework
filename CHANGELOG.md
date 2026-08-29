# Changelog

All notable changes to **rust-agent-framework** are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.2] — 2026-08-16 — 首次公开 crates.io 发布 · First public crates.io release

> **English** · **简体中文**

### English

The `rust-agent-framework` crate family is published to crates.io for the first time,
now managed under the `Rust-Framework` organization.

### Added

- **Core**: `rust-agent-core` / `rust-agent-client` / `rust-agent-framework` / `rust-agent-macros`
- **Capabilities**: `rust-agent-decl` / `rust-agent-rhai` / `rust-websearch` / `rust-agent-websearch`
- **Orchestration**: `rust-agent-workflow` / `rust-agent-workflow-pro` / `rust-agent-host`
- **Ecosystem**: `rust-agent-rag` / `rust-agent-wiki` / `rust-agent-mcp` / `rust-agent-openapi`
- **Runtime**: `rust-agent-coding` / `rust-agent-sandbox` / `rust-agent-llama`
- **Automated publishing**: new GitHub Actions `publish.yml` publishes crates in dependency order on `v*` tag push.

---

### 简体中文

`rust-agent-framework` 全系 16 个 crate 首次发布到 crates.io，归入 `Rust-Framework`
组织统一管理。

### 新增

- **核心**：`rust-agent-core` / `rust-agent-client` / `rust-agent-framework` / `rust-agent-macros`
- **能力**：`rust-agent-decl` / `rust-agent-rhai` / `rust-websearch` / `rust-agent-websearch`
- **编排**：`rust-agent-workflow` / `rust-agent-workflow-pro` / `rust-agent-host`
- **生态**：`rust-agent-rag` / `rust-agent-wiki` / `rust-agent-mcp` / `rust-agent-openapi`
- **运行时**：`rust-agent-coding` / `rust-agent-sandbox` / `rust-agent-llama`
- **自动化发布**：新增 GitHub Actions `publish.yml`，推送 `v*` tag 时按依赖顺序自动发布。

[0.1.2]: https://github.com/Rust-Framework/rust-agent-framework/releases/tag/v0.1.2