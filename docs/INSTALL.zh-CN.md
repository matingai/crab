# 安装 Crab

Crab 可以从源码安装，也可以从 GitHub Release CLI 压缩包安装。0.1.x 早期阶段源码安装仍然最可靠，
但 release 压缩包可以避免首次长时间编译。

## 环境要求

- Rust 1.85 或更新版本。
- Git。
- 只有执行真实模型请求时才需要模型 provider。无密钥 smoke test 不需要。
- 只有运行桌面壳时才需要 Node.js 和 npm。
- 只有使用当前 PDF 检查和抽取工具时才需要 `PATH` 中有 Swift。

## 从 GitHub Release 安装

tagged release 可以发布这些 CLI 压缩包：

| 平台 | 文件 |
| --- | --- |
| macOS Apple Silicon | `crab-vX.Y.Z-aarch64-apple-darwin.tar.gz` |
| macOS Intel | `crab-vX.Y.Z-x86_64-apple-darwin.tar.gz` |
| Linux x64 | `crab-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz` |
| Windows x64 | `crab-vX.Y.Z-x86_64-pc-windows-msvc.zip` |

macOS 或 Linux 可以从 `https://github.com/matingai/crab/releases` 下载匹配的 asset，
然后安装二进制：

```bash
VERSION=v0.1.0
TARGET=aarch64-apple-darwin
curl -LO "https://github.com/matingai/crab/releases/download/${VERSION}/crab-${VERSION}-${TARGET}.tar.gz"
tar -xzf "crab-${VERSION}-${TARGET}.tar.gz"
sudo install -m 0755 "crab-${VERSION}-${TARGET}/crab" /usr/local/bin/crab
crab --help
```

Windows 下载 `.zip` 后解压，把解压目录加入 `PATH`，或者把 `crab.exe` 移到已有的 `PATH`
目录中。

每个 release 压缩包也会包含对应的 `.sha256` 校验文件。

## 从 GitHub 源码安装

可以直接从公开仓库安装 CLI：

```bash
cargo install --git https://github.com/matingai/crab.git --locked
```

第一次源码构建可能需要几分钟，因为 Crab 包含浏览器、PDF、Office 和本地 runtime 相关依赖。

验证二进制：

```bash
crab --help
crab debug-context --prompt "Explain how Crab tracks goals and delegates work."
```

第二条命令不会请求模型，只会打印 Crab 会发给模型的上下文。

## 从本地 checkout 安装

如果要开发或本地改代码：

```bash
git clone https://github.com/matingai/crab.git
cd crab
cargo install --path . --locked
```

和 GitHub 安装路径一样，第一次本地 release 构建可能需要几分钟。

也可以不安装，直接运行：

```bash
cargo run -- debug-context --prompt "Explain the runtime architecture."
cargo run -- chat
```

## 构建本地 Release 压缩包

为当前机器生成可安装压缩包：

```bash
scripts/package-release.sh
```

脚本会写出 `dist/crab-v<version>-<target>.tar.gz` 或 `.zip`，并生成 `.sha256` 校验文件。
可以用 `CRAB_VERSION` 或 `CRAB_TARGET` 覆盖默认版本或 target。

## 配置模型 Provider

Crab 接受 OpenAI-compatible endpoint：

```bash
export OPENAI_API_KEY="your-api-key"
export OPENAI_BASE_URL="https://api.openai.com/v1"
export HERMES_RS_MODEL="gpt-4.1-mini"
```

如果使用 Cockpit、NewAPI 或本地网关，把 `OPENAI_BASE_URL` 指向网关兼容 OpenAI 的 `/v1`
endpoint，并把 `HERMES_RS_MODEL` 设置为对应的路由模型名。

## 运行桌面壳

桌面壳目前还没有作为独立 app 打包发布，需要从源码运行：

```bash
cd desktop-shell
npm install
npm run electron:dev
```

如果只预览 renderer：

```bash
cd desktop-shell
npm run dev
```

打开 `http://localhost:1420`。

## 本地状态

Crab 当前会把本地 runtime 状态存到：

```text
<workspace>/.hermes-agent-rs
```

该目录已经被 Git 忽略，里面可能包含 session、memory、archive、provider 配置和模型输出。不要提交它。

## 排障

- 如果 `cargo install` 失败，先检查 `rustc --version`，并升级到 Rust 1.85 或更新版本。
- 如果模型请求失败，检查 `OPENAI_API_KEY`、`OPENAI_BASE_URL` 和 `HERMES_RS_MODEL`。
- 如果 PDF 检查或抽取失败，确认同一个终端里 `swift --version` 可以正常运行。
- 如果桌面壳启动失败，在 `desktop-shell/` 里重新运行 `npm install`，并确认 Node.js 可用。
- 除非明确需要，否则保持 terminal 工具关闭。只在可信工作区使用 `--enable-shell`。
