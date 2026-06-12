# 安装 Crab

Crab 可以作为桌面 app、独立 CLI 二进制，或者从源码安装。0.1.x 早期阶段，源码安装最适合贡献者
排查细节；release 安装包和压缩包更适合第一次试用的用户。

## 环境要求

- Rust 1.85 或更新版本。
- Git。
- 只有执行真实模型请求时才需要模型 provider。无密钥 smoke test 不需要。
- 只有开发或本地打包桌面壳时才需要 Node.js 和 npm。
- 只有使用当前 PDF 检查和抽取工具时才需要 `PATH` 中有 Swift。

## 安装桌面 App

从 `https://github.com/matingai/crab/releases` 下载匹配的安装包：

| 平台 | 文件 |
| --- | --- |
| macOS Apple Silicon | `crab-desktop-vX.Y.Z-aarch64-apple-darwin.dmg` |
| macOS Intel | `crab-desktop-vX.Y.Z-x86_64-apple-darwin.dmg` |
| Windows x64 | `crab-desktop-vX.Y.Z-x86_64-pc-windows-msvc-setup.exe` |

macOS 打开 DMG 后把 Crab 拖入 Applications。Windows 直接运行 setup `.exe`。

当前 0.1.x 桌面安装包是未签名 preview build。macOS Gatekeeper 或 Windows SmartScreen 可能在首次
启动前提示风险。测试 GitHub release 产物时，建议同时校验对应 `.sha256` 文件。每个桌面安装包
还会带一个同名 `.json` manifest，记录 target triple、bundle 类型、文件名和 SHA-256，方便后续下载页、
镜像或自动安装逻辑使用。

macOS 校验：

```bash
shasum -a 256 -c crab-desktop-vX.Y.Z-aarch64-apple-darwin.dmg.sha256
```

Windows PowerShell 校验：

```powershell
Get-FileHash .\crab-desktop-vX.Y.Z-x86_64-pc-windows-msvc-setup.exe -Algorithm SHA256
Get-Content .\crab-desktop-vX.Y.Z-x86_64-pc-windows-msvc-setup.exe.sha256
```

打开安装包前，两个 SHA-256 值应该一致。

## 从 GitHub Release 安装 CLI

tagged release 可以发布这些 CLI 压缩包：

| 平台 | 文件 |
| --- | --- |
| macOS Apple Silicon | `crab-vX.Y.Z-aarch64-apple-darwin.tar.gz` |
| macOS Intel | `crab-vX.Y.Z-x86_64-apple-darwin.tar.gz` |
| Linux x64 | `crab-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz` |
| Windows x64 | `crab-vX.Y.Z-x86_64-pc-windows-msvc.zip` |

### 一条命令安装

macOS 或 Linux：

```bash
curl -fsSL https://raw.githubusercontent.com/matingai/crab/main/scripts/install.sh | bash
```

Windows PowerShell：

```powershell
irm https://raw.githubusercontent.com/matingai/crab/main/scripts/install.ps1 | iex
```

安装脚本会跟随最新 GitHub release，包括 0.1.x 阶段的 pre-release。可以用
`CRAB_VERSION` 指定 release tag，用 `CRAB_INSTALL_DIR` 指定安装目录。

### 手动安装

macOS 或 Linux 可以从 `https://github.com/matingai/crab/releases` 下载匹配的 asset，
然后安装二进制：

```bash
VERSION=v0.1.4
TARGET=aarch64-apple-darwin
curl -LO "https://github.com/matingai/crab/releases/download/${VERSION}/crab-${VERSION}-${TARGET}.tar.gz"
tar -xzf "crab-${VERSION}-${TARGET}.tar.gz"
sudo install -m 0755 "crab-${VERSION}-${TARGET}/crab" /usr/local/bin/crab
crab --help
crab --version
```

Windows 下载 `.zip` 后解压，把解压目录加入 `PATH`，或者把 `crab.exe` 移到已有的 `PATH`
目录中。

每个 release asset 都会有对应的 `.sha256` 校验文件。

## 从 GitHub 源码安装

可以直接从公开仓库安装 CLI：

```bash
cargo install --git https://github.com/matingai/crab.git --locked
```

第一次源码构建可能需要几分钟，因为 Crab 包含浏览器、PDF、Office 和本地 runtime 相关依赖。

验证二进制：

```bash
crab --help
crab doctor
crab debug-context --prompt "Explain how Crab tracks goals and delegates work."
```

`doctor` 会检查本地 workspace、runtime、provider 配置、shell 安全开关、release 脚本和可选开发工具，
不会打印密钥。`debug-context` 不会请求模型，只会打印 Crab 会发给模型的上下文。

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
cargo run -- doctor
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

## 从源码运行或打包桌面壳

从源码运行桌面壳：

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

构建本地 Tauri 安装包：

```bash
cd desktop-shell
npm run package:desktop
```

macOS 下可以用 `npm run package:dmg` 把 DMG 写入 `dist/`。Windows 下可以用
`npm run package:exe` 生成 NSIS setup `.exe`。脚本也会生成同名 `.sha256` 校验文件和
`.json` manifest。需要显式指定 Tauri target 时可以设置 `CRAB_TARGET`，例如：

```bash
CRAB_TARGET=aarch64-apple-darwin scripts/package-desktop.sh
```

CI asset 命名和签名说明见 [桌面安装包文档](DESKTOP_PACKAGING.zh-CN.md)。

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
