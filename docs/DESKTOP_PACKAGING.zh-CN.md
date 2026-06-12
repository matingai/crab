# 桌面安装包

Crab 同时发布 CLI 和桌面壳。CLI 压缩包适合开发者、服务器和脚本化工作流；桌面安装包面向普通用户：
下载、安装、打开应用、选择工作区，然后用可视化界面观察 agent loop。

英文版：[Desktop Packaging](DESKTOP_PACKAGING.md)。

## 用户安装路径

大多数用户不需要从源码构建 Crab。发布 tag 之后，可以让用户直接去
[GitHub Releases](https://github.com/matingai/crab/releases) 下载对应平台的桌面安装包：

- macOS Apple Silicon：`crab-desktop-vX.Y.Z-aarch64-apple-darwin.dmg`
- macOS Intel：`crab-desktop-vX.Y.Z-x86_64-apple-darwin.dmg`
- Windows x64：`crab-desktop-vX.Y.Z-x86_64-pc-windows-msvc-setup.exe`

安装体验应该尽量普通、直觉：

- macOS：打开 DMG，把 Crab 拖进 Applications。
- Windows：运行 setup `.exe`。

当前 0.1.x 安装包是未签名 preview build。在接入 macOS notarization 和 Windows
Authenticode 签名之前，release notes 里需要持续提示 Gatekeeper / SmartScreen 可能告警。

## Release 产物

tagged release 会构建这些桌面安装包：

| 平台 | 文件 |
| --- | --- |
| macOS Apple Silicon | `crab-desktop-vX.Y.Z-aarch64-apple-darwin.dmg` |
| macOS Intel | `crab-desktop-vX.Y.Z-x86_64-apple-darwin.dmg` |
| Windows x64 | `crab-desktop-vX.Y.Z-x86_64-pc-windows-msvc-setup.exe` |

每个安装包旁边都会有同名 `.sha256` 校验文件。桌面安装包还会生成一个很小的 `.json`
manifest，包含文件名、版本、target triple、bundle 类型和 SHA-256，方便未来下载页、镜像或自动更新逻辑使用。

## 本地构建

先安装桌面壳依赖：

```bash
cd desktop-shell
npm install
```

正式跑原生打包之前，先跑快速预检：

```bash
cd desktop-shell
npm run release:check
```

预检会检查版本是否对齐、Tauri bundle 元数据是否齐全、DMG 和 NSIS 覆盖是否存在、桌面图标是否齐、
packaging helper 是否仍然生成 checksum/manifest，以及 GitHub Release workflow 是否包含桌面安装包矩阵。
它也会直接打印下一次 tag 会发布的桌面 asset 名称。

为当前平台构建安装包：

```bash
cd desktop-shell
npm run package:desktop
```

在 macOS 上构建 DMG：

```bash
cd desktop-shell
npm run package:dmg
```

在 Windows 上构建 NSIS setup `.exe`：

```bash
cd desktop-shell
npm run package:exe
```

构建脚本会把 release-ready 文件写到仓库根目录的 `dist/`，例如
`crab-desktop-v0.1.4-aarch64-apple-darwin.dmg`，并生成配套 `.sha256` 和 `.json`
metadata。Tauri 原始 bundle 仍会保留在 `desktop-shell/src-tauri/target/release/bundle/`。

需要显式指定 target 时，可以设置环境变量：

```bash
CRAB_TARGET=aarch64-apple-darwin scripts/package-desktop.sh
```

也可以使用参数：

```bash
scripts/package-desktop.sh --target aarch64-apple-darwin --bundle dmg --version v0.1.4
```

CI 会为每个平台传入明确 target。显式 target 的 Tauri 输出目录在
`desktop-shell/src-tauri/target/<target>/release/bundle/`。

## CI 构建

`.github/workflows/release.yml` 会在 GitHub 原生 runner 上构建桌面安装包：

- `macos-14`：Apple Silicon DMG。
- `macos-15-intel`：Intel Mac DMG。
- `windows-2025`：Windows x64 NSIS setup `.exe`。

release workflow 使用 `scripts/package-desktop.sh`，因此本地和 CI 的文件命名、SHA-256
行为、manifest 结构保持一致。
workflow 还会在原生打包前运行 `scripts/check-desktop-release.mjs`，让版本漂移或打包配置缺失尽早失败。

每个桌面安装包应包含：

- 安装包本体（`.dmg` 或 setup `.exe`）；
- 同名 `.sha256` 校验文件；
- 同名 `.json` manifest。

## 签名与信任

当前 0.1.x 桌面安装包是未签名 preview build，可以用于测试，但系统可能会提示风险：

- macOS 可能出现 Gatekeeper 提示，因为 DMG 还没有 Developer ID 签名和 notarization。
- Windows 可能出现 SmartScreen 提示，因为 setup `.exe` 还没有 Authenticode 签名。

面向正式用户的桌面发布应继续补齐：

- Apple Developer ID 签名；
- macOS notarization 和 stapling；
- Windows Authenticode 签名；
- 每个安装包的 checksum 校验说明。

## 校验安装包

macOS 或 Linux：

```bash
shasum -a 256 -c crab-desktop-vX.Y.Z-aarch64-apple-darwin.dmg.sha256
```

Windows PowerShell：

```powershell
Get-FileHash .\crab-desktop-vX.Y.Z-x86_64-pc-windows-msvc-setup.exe -Algorithm SHA256
Get-Content .\crab-desktop-vX.Y.Z-x86_64-pc-windows-msvc-setup.exe.sha256
```

打开安装包之前，两个 SHA-256 值应该一致。

## 版本对齐

桌面安装包版本需要和以下文件保持一致：

- 根目录 `Cargo.toml`；
- `desktop-shell/package.json`；
- `desktop-shell/src-tauri/Cargo.toml`；
- `desktop-shell/src-tauri/tauri.conf.json`。

打 tag 之前，先跑一遍 [Release Process](RELEASE_PROCESS.md) 里的检查清单。
