# Crab 常见问题

## 试用 Crab 一定需要 API key 吗？

不需要。第一次 smoke test 可以先用 `doctor` 检查本地环境且不暴露密钥，再用 `debug-context`
查看 Crab 会如何组装 prompt、runtime profile、skills、memory snapshot、goal-state digest
和工具定义，不会真实请求模型：

```bash
cargo run -- doctor
cargo run -- debug-context --prompt "Explain how Crab tracks goals and delegates work."
```

只有在执行真实模型回复时才需要 provider，例如 `cargo run -- chat --prompt "..."`
或 `debug-context --execute`。

## 如何安装 Crab？

从 GitHub 安装：

```bash
cargo install --git https://github.com/matingai/crab.git --locked
```

然后运行 `crab doctor`。本地 checkout、provider 和桌面壳配置见 [安装 Crab](INSTALL.zh-CN.md)。

## 可以用 Cockpit、NewAPI、本地网关或本地模型吗？

可以，只要它暴露 OpenAI-compatible API。用环境变量配置 endpoint 和模型：

```bash
export OPENAI_API_KEY="your-gateway-key"
export OPENAI_BASE_URL="https://your-gateway.example.com/v1"
export HERMES_RS_MODEL="your-routed-model"
```

本地网关也可以使用类似 `http://127.0.0.1:11434/v1` 的地址，只要它兼容 Chat
Completions 或 Responses 风格的接口。

## Crab 最核心的想法是什么？

Crab 把 agent loop 当成产品核心。主模型像一个面向目标求解的控制器，负责追踪目标、阻塞点、
证据、风险、置信度和下一步。工具和 worker runs 负责产出边界清晰的观察结果，主循环再把这些结果合并回本地状态。

## Crab 和普通 chat wrapper 有什么不同？

普通 chat wrapper 通常只是把用户输入发给模型，再渲染回复。Crab 会围绕对话维护 runtime state：
session、goal state、memory、skills、todos、approval request、工具观察、worker 委派记录和 bridge
事件。桌面壳可以展示执行时间线，而不是只显示一个最终回答。

## worker 委派是什么意思？

worker 委派指的是主循环可以把边界清晰的子任务交给另一个 run 或辅助模型，例如“检查文档里的隐私声明”
或“阅读某个代码区域并返回证据”。主模型仍然负责调度和最终综合，worker 只产出聚焦的发现。

## Crab 现在能用于生产吗？

不能。Crab 目前是活跃开发中的 0.1.x 原型，适合实验、架构讨论和本地工作流试用。公开 API、
事件格式、桌面行为和本地数据布局都可能继续变化。

## 在私有工作区运行安全吗？

请把它当成实验性的本地自动化工具。terminal 工具默认关闭，敏感操作可以走 approval，但项目没有经过正式安全审计。
建议只在可信工作区使用，审阅模型输出，不要提交 `.hermes-agent-rs/` 或 `.env`，除非明确需要，否则不要启用 shell。

## Crab 的本地状态存在哪里？

当前兼容数据目录是：

```text
<workspace>/.hermes-agent-rs
```

它可能包含 session、archive、memory、runtime 数据、provider 配置和模型输出。该目录已经被 Git 忽略，
未来 breaking release 可能迁移到 Crab 命名路径。

## 为什么二进制叫 `crab`，但有些环境变量还是 `HERMES_RS_*`？

项目已经改名为 Crab，但 0.1.x 阶段保留部分 `HERMES_RS_*` 环境变量和兼容路径，避免不必要的破坏性变化。
未来 breaking release 可以更完整地迁移命名。

## 应该使用哪个桌面壳？

当前实际可用路径是 Next.js + Electron。仓库也保留了 Tauri 骨架，方便后续 native integration，
但现在 Electron 路径更完整。

## 应该怎么反馈？

关于 goal state、worker 委派、工具证据、approval 边界和本地状态的设计反馈，可以使用
`Agent loop feedback` issue template。可复现问题用 bug report，新能力建议用 feature request。

## 最推荐的第一个 demo 是什么？

先运行无密钥上下文预览：

```bash
cargo run -- debug-context --prompt "Explain Crab's agent loop and worker delegation design."
```

再运行桌面 renderer：

```bash
cd desktop-shell
npm install
npm run dev
```

打开 `http://localhost:1420`，查看首次启动的 Agent Loop demo 态。
