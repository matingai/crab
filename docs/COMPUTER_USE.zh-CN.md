# Computer Use / 电脑使用

Crab 的 computer-use 层是 browser tools 的原生桌面对照能力。Browser tools 操作的是一个受控
网页会话；computer-use tools 面向真实桌面，需要操作系统权限，尤其是 macOS 的辅助功能
（Accessibility）授权。

当前实现刻意保持保守：

- 通过 macOS ApplicationServices 原生 API 检测 Accessibility 授权状态。
- 提供首次设置时的授权弹窗入口。
- 在已授权时读取前台应用和窗口的浅层快照。
- 暂不开放鼠标、键盘、文件或应用控制等写操作。

这是有意的边界。系统级自动化不应该作为普通聊天的隐式副作用出现，而应该经过明确权限、可观察
tool call 和 approval policy。

## 工具接口

内置 `computer_use` 工具有三个 action：

| Action | 行为 |
| --- | --- |
| `status` | 返回平台支持、Accessibility 授权、是否支持弹窗以及设置指引。 |
| `request_permission` | 调用 macOS Accessibility prompt API，并返回当前状态。 |
| `snapshot` | 读取前台应用及窗口的紧凑轮廓。 |

示例参数：

```json
{
  "action": "status"
}
```

```json
{
  "action": "snapshot",
  "max_items": 10
}
```

## macOS 授权流程

在 macOS 上，Accessibility 权限属于启动 Crab 的进程。本地开发时通常是 Terminal、iTerm 或
Electron/Tauri 桌面应用；正式打包后一般会显示为 Crab。

启用方式：

1. 调用 `computer_use`，参数为 `action=request_permission`。
2. 打开系统设置。
3. 进入 Privacy & Security > Accessibility。
4. 勾选 Crab 或启动它的终端。
5. 如果 macOS 没有立刻刷新授权状态，重启应用或终端。

`crab doctor` 也会报告这个可选能力。未授权会显示为 warning，但不会阻断核心 runtime。

## 安全模型

第一个里程碑是只读能力：让 agent 知道原生桌面自动化是否可用，并得到一个可审计的桌面快照。
后续如果加入点击、输入等写操作，应该继续由这些边界保护：

- 明确的工具名和参数；
- 本地 `tool_policy` approval 规则；
- 已脱敏的 event 和 archive 记录；
- 桌面 timeline 中可见的执行事件；
- 每个平台执行前的权限检查。

这样 computer use 才和 Crab 的 agent-loop 理念一致：模型可以理解桌面，但 runtime 仍然负责权限、
证据和行动边界。
