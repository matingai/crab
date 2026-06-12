# Computer Use / 电脑使用

Crab 的 computer-use 层是 browser tools 的原生桌面对照能力。Browser tools 操作的是一个受控
网页会话；computer-use tools 面向真实桌面，需要操作系统权限，尤其是 macOS 的辅助功能
（Accessibility）授权。

当前实现刻意保持保守：

- 通过 macOS ApplicationServices 原生 API 检测 Accessibility 授权状态。
- 提供首次设置时的授权弹窗入口。
- 在已授权时读取前台应用的 Accessibility UI tree，并为元素生成 `@u1` 这类引用。
- 支持经过 approval 的当前 `@u` ref 点击。
- 暂不开放键盘文本输入、文件或宽泛应用控制等写操作。

这是有意的边界。系统级自动化不应该作为普通聊天的隐式副作用出现，而应该经过明确权限、可观察
tool call 和 approval policy。

## 工具接口

内置 `computer_use` 工具有四个 action：

| Action | 行为 |
| --- | --- |
| `status` | 返回平台支持、Accessibility 授权、是否支持弹窗以及设置指引。 |
| `request_permission` | 调用 macOS Accessibility prompt API，并返回当前状态。 |
| `snapshot` | 读取前台应用及窗口的紧凑 Accessibility UI tree。 |
| `click` | 激活 `@u2` 这类 snapshot ref，然后返回点击后的快照。 |

示例参数：

```json
{
  "action": "status"
}
```

```json
{
  "action": "snapshot",
  "max_items": 40,
  "max_depth": 3
}
```

snapshot 输出包含前台应用名称、进程 id 和有界 UI tree。每一行可见元素都会带上本次快照内稳定的
引用，并尽量包含 role、name、value 和 bounds：

```text
frontmost_app: Finder
pid: 123
ui_tree:
- @u1 role='window' name='Documents' bounds=(80,80,900x640)
  - @u2 role='button' name='Back' bounds=(94,96,28x28)
```

当前里程碑中这些 refs 是观察句柄。它们的设计目标是让经过 approval 的动作可以定位到具体元素，
而不是猜屏幕坐标。

click ref 是短期有效的：先获取最新 `snapshot`，从输出里选择一个可见的 `@u` 引用，然后立刻调用
`click`。如果应用在点击前发生变化，这个 ref 可能会解析到新 UI tree 里的另一个元素。

```json
{
  "action": "click",
  "ref": "@u2",
  "max_items": 40,
  "max_depth": 3
}
```

`click` 是写动作。即使用户没有配置自定义 `tool_policy`，Crab 默认也会在执行前要求 approval。
只读 action 仍然可以直接运行。

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

第一个里程碑基本是只读能力，并把第一条写路径限制在经过 approval 的 observed ref 点击上。它让
agent 知道原生桌面自动化是否可用，并得到一个可审计的桌面 UI tree。后续如果加入输入等更多写操作，
应该继续由这些边界保护：

- 明确的工具名和参数；
- 本地 `tool_policy` approval 规则；
- 已脱敏的 event 和 archive 记录；
- 桌面 timeline 中可见的执行事件；
- 每个平台执行前的权限检查。

这样 computer use 才和 Crab 的 agent-loop 理念一致：模型可以理解桌面，但 runtime 仍然负责权限、
证据和行动边界。
