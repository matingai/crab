# Crab 快速上手

这份指南给第一次看到 Crab 的用户准备：先完成本地自检和无密钥 smoke test，最后再进入真实模型调用。

## 1. 安装或从源码运行

安装最新版 macOS/Linux CLI：

```bash
curl -fsSL https://raw.githubusercontent.com/matingai/crab/main/scripts/install.sh | bash
```

或者从源码运行：

```bash
git clone https://github.com/matingai/crab.git
cd crab
cargo run -- doctor
```

## 2. 运行本地自检

```bash
crab doctor
```

`doctor` 会检查当前 workspace、本地状态目录、模型 endpoint、shell 安全开关、release 脚本、
`.gitignore` 卫生情况，以及 Rust/Node/Swift 等可选工具链。它不会请求模型，也不会打印任何 API key。

如果要把结果放到 issue 或自动化里：

```bash
crab doctor --json
```

## 3. 无密钥查看 Agent Loop

```bash
crab debug-context --prompt "Explain how Crab tracks goals and delegates work."
```

这个命令会打印 Crab 准备发给模型的上下文，包括 system prompt、workspace instructions、
goal-state digest、memory snapshot、runtime profile 和工具定义。它适合第一次试用、录屏、
演示和 CI-safe smoke test。

## 4. 配置模型 Provider

Crab 接受 OpenAI-compatible endpoint：

```bash
export OPENAI_API_KEY="your-api-key"
export OPENAI_BASE_URL="https://api.openai.com/v1"
export HERMES_RS_MODEL="gpt-4.1-mini"
```

如果使用 Cockpit、NewAPI 或其他网关，变量仍然保持同样形状，只需要把 `OPENAI_BASE_URL`
指向网关的 `/v1` endpoint。

再次检查：

```bash
crab doctor
```

当 key 已经通过环境变量或被忽略的本地配置提供后，模型 key 的 warning 应该消失。

## 5. 跑第一个 Prompt

```bash
crab chat --prompt "Read README.md and summarize Crab's agent-loop design in five bullets."
```

如果是在可信 coding workspace 中，需要开启 shell 工具：

```bash
crab --enable-shell chat --prompt "Inspect the repository and propose one safe improvement."
```

不可信目录里请保持 shell 关闭。

## 6. 打开桌面预览

在源码 checkout 中运行：

```bash
cd desktop-shell
npm install
npm run dev
```

打开 `http://localhost:1420`。第一次进入时可以用内置 demo state 展示 timeline、runtime settings、
skills 和 agent-loop 故事，不需要马上连接真实 workspace。

## 7. 推荐演示路径

适合公开录屏或直播的顺序：

1. 运行 `crab doctor`。
2. 运行 `crab debug-context --prompt "Explain Crab's agent loop and worker delegation."`。
3. 打开桌面预览。
4. 使用 demo credentials 跑一个 [examples](../examples/README.md) 里的工作流。
5. 引导观众阅读 [Agent Loop](AGENT_LOOP.md)、[Architecture](ARCHITECTURE.md) 和
   [Future Vision](FUTURE_VISION.md)。
