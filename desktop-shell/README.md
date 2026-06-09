# Hermes Agent RS Desktop Shell

这个目录现在是 `Next.js App Router + shadcn 风格组件` 的桌面壳前端，当前同时保留：

- `Tauri shell`
- `Electron shell`

## 结构

- `src-tauri/`: Tauri desktop backend
- `electron/`: Electron main/preload/dev 脚本
- `app/`: Next.js App Router 页面
- `components/ui/`: 本地 shadcn 风格基础组件
- `lib/`: 前端工具函数和桌面桥抽象
- `package.json`: Next.js、Tailwind、`@tauri-apps/api`、Electron 和脚本入口

## 当前已接上的后端能力

- `run_agent`
- `resume_approval`
- `run_cron_job`
- `retry_delegate_run`
- `clear_session`
- `stop_session`
- `list_approvals`
- `resolve_approval`
- `list_skills`
- `view_skill`
- `list_providers`
- `resolve_provider_status`
- `list_sessions`
- `load_session`
- `list_delegate_runs`
- `cancel_delegate_run`
- `extensions_overview`
- `save_cron_job`
- `delete_cron_job`
- `inspect_mcp_server`
- `browser_stream_endpoint`
- `view_workspace_file`
- `resolve_runtime_profile`
- `resolve_runtime_status`
- `start_runtime`
- `repair_runtime`
- `reset_runtime`
- `desktop_info`

## 事件约定

- `hermes://agent/event`
- `hermes://agent/done`
- `hermes://agent/cleared`
- `hermes://agent/event/<session_id>`
- `hermes://agent/done/<session_id>`

## 说明

当前前端已经统一走 `lib/desktop.ts` 的桌面桥抽象：

- 在 Tauri 下动态转发到 `@tauri-apps/api`
- 在 Electron 下转发到 preload 暴露的 `window.hermesDesktop`

这让桌面壳迁移可以分阶段做，而不需要一口气重写页面。

## 启动

先装依赖：

```bash
cd desktop-shell
npm install
```

Electron 开发态：

```bash
cd desktop-shell
npm run electron:dev
```

这条命令会先拉起 `http://localhost:1420` 的 Next dev server，再启动 Electron 窗口。

Electron 发布态原型：

```bash
cd desktop-shell
npm run electron:release
```

如果只想单独调前端：

```bash
cd desktop-shell
npm run dev
```

Tauri 开发态：

```bash
cd desktop-shell
npm run tauri:dev
```

兼容旧命名的别名仍然保留：

```bash
cd desktop-shell
npm run desktop:dev
```

这条命令会先拉起 `http://localhost:1420` 的 Next dev server，再启动 Tauri 窗口。不要直接在开发态执行 `cargo run --manifest-path src-tauri/Cargo.toml`，否则窗口会去找 `devUrl`，前端没起来时就会白屏。

Tauri 发布态：

```bash
cd desktop-shell
npm run tauri:release
```

注意：

- Electron 现在会通过 Rust `desktop-bridge` 子命令转发大部分 agent/session/runtime 命令
- 开发态默认优先复用 `../target/debug/hermes-agent-rs`，不存在时回退到 `cargo run -- desktop-bridge`
- 可以通过环境变量 `HERMES_RUST_BRIDGE_BIN` 指定自定义 Rust bridge 可执行文件
- `pick_workspace_folder` 和 `list_workspace_tree` 仍由 Electron 主进程本地实现
- `view_workspace_file` 现在也会走 Rust bridge，所以 Electron 下可以复用 Office 运行时转 PDF 预览
- `start_cron_scheduler` / `stop_cron_scheduler` 现在由 Electron 主进程维护真实调度循环，到期任务仍通过 Rust bridge 执行

Tauri backend 检查：

```bash
cd hermes-agent-rs
cargo check --manifest-path desktop-shell/src-tauri/Cargo.toml
```
