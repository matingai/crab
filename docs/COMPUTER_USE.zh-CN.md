# Computer Use / 电脑使用

Crab 的 computer-use 层是 browser tools 的原生桌面对照能力。Browser tools 操作的是一个受控
网页会话；computer-use tools 面向真实桌面，需要操作系统权限，尤其是 macOS 的辅助功能
（Accessibility）授权。

当前实现刻意保持保守：

- 通过 macOS ApplicationServices 原生 API 检测 Accessibility 授权状态。
- 提供首次设置时的授权弹窗入口。
- 在已授权时读取前台应用的 Accessibility UI tree，并为元素生成 `@u1` 这类引用。
- 支持对当前 ref 做只读检查，返回当前元素行和系统报告的原生 Accessibility actions。
- 支持只读等待某个当前 ref 存在，并匹配 role、文本、状态或原生 Accessibility action 预期。
- 支持在新的 Accessibility snapshot 里按文本、role、紧凑状态和系统报告的原生 Accessibility action
  只读搜索候选 ref。
- 只读等待指定文本出现、消失，或等待前台 Accessibility tree 稳定。
- 支持写动作前的可选 ref guard：先检查当前 ref 的 role、文本或紧凑状态，再执行经过 approval 的写动作。
- 支持经过 approval 的当前 `@u` ref 聚焦。
- 支持经过 approval 的当前 `@u` ref 点击。
- 支持经过 approval 的小范围原生 Accessibility action，例如 press、show menu、confirm、cancel、increment 和 decrement。
- 当目标 Accessibility element 支持可写 value 时，支持经过 approval 的当前 `@u` ref 文本设置。
- 支持通过原生 Accessibility scroll action 对当前 `@u` ref 做经过 approval 的小步滚动。
- 支持经过 approval 的小范围非文本按键，例如 Enter、Escape、Tab、方向键和翻页键。
- 暂不开放任意全局键盘输入、文件或宽泛应用控制等写操作。

这是有意的边界。系统级自动化不应该作为普通聊天的隐式副作用出现，而应该经过明确权限、可观察
tool call 和 approval policy。

## 工具接口

内置 `computer_use` 工具有十三个 action：

| Action | 行为 |
| --- | --- |
| `status` | 返回平台支持、Accessibility 授权、是否支持弹窗以及设置指引。 |
| `request_permission` | 调用 macOS Accessibility prompt API，并返回当前状态。 |
| `snapshot` | 读取前台应用及窗口的紧凑 Accessibility UI tree。 |
| `inspect_ref` | 读取 snapshot ref 的当前详情和系统报告的原生 Accessibility actions。 |
| `find` | 在新的 snapshot 里按 query、role 或 state 搜索候选 UI ref，并返回匹配元素行。 |
| `wait` | 轮询 snapshot，直到目标文本出现、消失或 UI tree 稳定，然后返回最新快照。 |
| `wait_ref` | 轮询单个 UI ref，直到它存在，并且可选 role、文本、状态或 native action 预期匹配。 |
| `focus` | 把键盘焦点设到 `@u2` 这类 snapshot ref，然后返回聚焦后的快照。 |
| `click` | 激活 `@u2` 这类 snapshot ref，然后返回点击后的快照。 |
| `perform_action` | 对 snapshot ref 执行一个白名单原生 Accessibility action，然后返回操作后的快照。 |
| `set_text` | 设置 snapshot ref 的 Accessibility value，然后返回操作后的快照。 |
| `scroll` | 对 snapshot ref 执行小步 Accessibility scroll，然后返回操作后的快照。 |
| `press_key` | 在前台应用里按下一个白名单非文本按键，然后返回操作后的快照。 |

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

snapshot 输出包含非敏感 record 元数据、前台应用名称、进程 id 和有界 UI tree。`snapshot`、`find`、
`wait`、`wait_ref` 和 `inspect_ref` 都会返回同一组 `snapshot_*` 元数据头，让 agent 可以在观察和行动
之间携带明确证据。每一行可见元素都会带上本次快照内稳定的引用，并尽量包含 role、name、value、bounds
和紧凑状态标记：

```text
snapshot_id: cu_7d3c0a5d21a9e472
snapshot_max_items: 40
snapshot_max_depth: 3
snapshot_sha256: 5b2f...
snapshot_app_line_sha256: 9d8e...
snapshot_pid: 123
frontmost_app: Finder
pid: 123
ui_tree:
- @u1 role='window' name='Documents' bounds=(80,80,900x640)
  - @u2 role='button' name='Back' bounds=(94,96,28x28) focused=true
  - @u3 role='button' name='Continue' bounds=(740,680,120x32) enabled=false
```

当前里程碑中这些 refs 是观察句柄。它们的设计目标是让经过 approval 的动作可以定位到具体元素，
而不是猜屏幕坐标。
snapshot 状态标记会刻意保持稀疏：`focused=true` 和 `selected=true` 只在存在时显示，
`enabled=false` 用来标记不可用控件，不会给每个正常可用元素增加噪声。

`inspect_ref` 是针对单个 observed ref 的只读预检查。它会重新读取当前前台应用，返回目标元素行和
`available_actions`，并保存一个新的 `snapshot_id`，供下一步动作继续使用：

```json
{
  "action": "inspect_ref",
  "ref": "@u8",
  "max_items": 40,
  "max_depth": 3
}
```

这样 agent 可以根据系统报告的原生 actions，在 `perform_action`、`click`、`scroll`、`set_text`
或按键流程之间做选择，而不是只靠 UI 文本猜下一步。

`wait_ref` 是针对单个 observed ref 的只读就绪检查。适合 agent 已经找到了目标控件，但需要等它
变为 enabled，或等 `AXPress` 这类 native action 出现之后，再请求 approval 执行写动作：

```json
{
  "action": "wait_ref",
  "ref": "@u8",
  "expect_role": "button",
  "expect_text": "Continue",
  "expect_state": "enabled",
  "native_action": "AXPress",
  "timeout_seconds": 10,
  "poll_interval_ms": 250,
  "max_items": 40,
  "max_depth": 3
}
```

无论 `wait_ref` 匹配还是超时，它都会返回该 ref 的最新详情和新的 `snapshot_id`。如果这个 ref
一直无法被检查，它会返回一个紧凑的 unavailable 标记和最后一次内部错误的 hash，而不是把失败路径里的
原始 UI 文本回显出来。

`find` 是原生 UI 工作流里的轻量定位步骤。它会读取新的 snapshot，保存新的 `snapshot_id`，
但只返回匹配到的元素行。适合 agent 已经知道要找什么、不需要再次输出完整 UI tree 的场景：

```json
{
  "action": "find",
  "query": "Continue",
  "role": "button",
  "state": "enabled",
  "native_action": "AXPress",
  "max_results": 12,
  "max_items": 40,
  "max_depth": 3
}
```

`query`、`role`、`state`、`native_action` 至少需要提供一个。`state` 支持 `focused`、`selected`、
`enabled` 和 `disabled`；其中 `enabled` 表示该 snapshot 行没有出现 `enabled=false`。如果提供
`native_action`，Crab 会对候选 ref 再做一次只读 details 检查，只返回当前在 `available_actions` 中报告了
该 action 的元素。返回的 `snapshot_id` 可以立即交给经过 approval 的 `focus`、`click`、`set_text` 或
`press_key` 动作使用；如果 agent 想先确认某个 ref 已经就绪，也可以继续交给只读 `wait_ref`。

`wait` 是原生 UI 工作流里的只读观察循环。无论条件匹配还是超时，它都会返回新的 `snapshot_id`
和最后一次 snapshot，方便下一步动作基于最新证据执行：

```json
{
  "action": "wait",
  "wait_until": "text_present",
  "contains_text": "Ready",
  "timeout_seconds": 10,
  "poll_interval_ms": 250,
  "max_items": 40,
  "max_depth": 3
}
```

```json
{
  "action": "wait",
  "wait_until": "text_absent",
  "contains_text": "Loading",
  "timeout_seconds": 10,
  "poll_interval_ms": 250,
  "max_items": 40,
  "max_depth": 3
}
```

```json
{
  "action": "wait",
  "wait_until": "settled",
  "timeout_seconds": 5,
  "max_items": 40,
  "max_depth": 3
}
```

action ref 是短期有效的：先获取最新 `snapshot`，从输出里选择一个可见的 `@u` 引用，然后立刻调用
`click` 或 `set_text`。如果应用在操作前发生变化，这个 ref 可能会解析到新 UI tree 里的另一个元素。
写动作会先校验最新的 `snapshot_id`、对应遍历边界和观察时间。如果省略 id，Crab 会使用当前 session
最近一次 snapshot 记录；如果传入的是旧 id，动作会失败，并要求 agent 重新观察桌面。因为 `@u` ref
是在有界 UI tree 里分配出来的，写动作还要求请求里的 `max_items` 和 `max_depth` 必须与生成该 ref 的
snapshot 一致。超过 30 秒的 snapshot 记录不能再用于写动作，避免 agent 把过期 UI 观察应用到已经变化
的桌面。Crab 还会把观察时的前台 app origin 记录成 hash 和 pid，并在每次写动作前自动重新校验；如果
焦点已经切到另一个 app 或进程，写动作会在任何 Accessibility 修改发生前被拒绝。写动作成功后，Crab 会
把返回的 post-action 观察结果保存为新的 latest snapshot record，并返回 `post_snapshot_id`，所以下一步
可以基于新鲜 UI 证据继续，而不是继续沿用操作前的 id。保存后的 post-action record 来自提取出的
`post_*_snapshot` 正文，不会把周围的动作日志行混进 snapshot 证据里。写动作输出还会给出 post
snapshot 的读取边界、输出 hash、前台 app line hash，以及可用时的 pid，让 agent 可以基于明确证据继续，
而不是猜隐藏状态。

为了让目标定位更稳，写动作还可以带上可选的 ref guard。`expect_role`、`expect_text` 和
`expect_state` 会让 Crab 在写动作前再读一次 snapshot，并验证选中的 ref 仍然像刚才观察到的控件。
如果 guard 失败，写动作不会执行，agent 应该重新调用 `snapshot` 或 `find`。

写动作也可以带上 `expect_app` 和 `expect_pid` 这类前台应用 guard。Crab 会在真正执行写动作前确认当前
前台应用仍然匹配预期。这个 guard 适合用户可能在观察和执行之间切换焦点的场景，尤其适合 `press_key`
这种作用在前台应用、而不是某个具体 ref 上的动作。guard 输出只包含当前 app line 的 hash 证据，不会回显
原始前台 app 文本。

```json
{
  "action": "click",
  "ref": "@u2",
  "snapshot_id": "cu_7d3c0a5d21a9e472",
  "expect_role": "button",
  "expect_text": "Continue",
  "expect_state": "enabled",
  "expect_app": "Finder",
  "expect_pid": 123,
  "max_items": 40,
  "max_depth": 3
}
```

```json
{
  "action": "focus",
  "ref": "@u5",
  "snapshot_id": "cu_7d3c0a5d21a9e472",
  "max_items": 40,
  "max_depth": 3
}
```

```json
{
  "action": "perform_action",
  "ref": "@u8",
  "snapshot_id": "cu_7d3c0a5d21a9e472",
  "native_action": "AXPress",
  "expect_role": "button",
  "expect_text": "Continue",
  "max_items": 40,
  "max_depth": 3
}
```

```json
{
  "action": "set_text",
  "ref": "@u5",
  "snapshot_id": "cu_7d3c0a5d21a9e472",
  "text": "hello",
  "max_items": 40,
  "max_depth": 3
}
```

```json
{
  "action": "scroll",
  "ref": "@u8",
  "snapshot_id": "cu_7d3c0a5d21a9e472",
  "direction": "down",
  "scroll_steps": 2,
  "expect_role": "scroll area",
  "max_items": 40,
  "max_depth": 3
}
```

```json
{
  "action": "press_key",
  "key": "enter",
  "snapshot_id": "cu_7d3c0a5d21a9e472",
  "expect_app": "Finder",
  "max_items": 40,
  "max_depth": 3
}
```

`focus`、`click`、`perform_action`、`set_text`、`scroll` 和 `press_key` 都是写动作。即使用户没有配置自定义 `tool_policy`，Crab 默认也会
在执行前要求 approval。`status`、`snapshot`、`inspect_ref`、`find`、`wait` 和 `wait_ref` 仍然可以不经 approval 直接运行。
`set_text` 不发送全局 keystroke，而是尝试直接设置目标 Accessibility element 的 value，因此主要适合文本框和类似控件。

`perform_action` 只接受一个小范围原生 Accessibility action 白名单：`press`、`show_menu`、
`confirm`、`cancel`、`increment` 和 `decrement`。`AXPress`、`AXShowMenu` 这类 AX 前缀名称也会被归一化。
真正执行前，Crab 会重新读取该 ref 的详情，并确认当前元素仍然在 `available_actions` 中报告了所选
native action。写动作结果只会包含 `native_action_guard_details_sha256` 这类证据 hash，不会回显原始
ref 详情。尽量先用 `inspect_ref` 或 `wait_ref` 获取当前 `available_actions`，再选择要执行的
native action。

`scroll` 会作用在一个具体 observed ref 上，只接受 `up`、`down`、`left`、`right` 四个方向，
并把 `scroll_steps` 限制在 `1..=10`。它适合在观察之后移动列表、scroll area、表格和面板，
不是任意全局鼠标滚轮注入。

`press_key` 刻意只接受一个很小的白名单：`enter`、`escape`、`tab`、`space`、`backspace`、
`forward_delete`、`arrow_up`、`arrow_down`、`arrow_left`、`arrow_right`、`page_up`、
`page_down`、`home` 和 `end`。它适合焦点 UI 的导航、确认和退出流程，不用于任意文本输入。

session snapshot 记录会刻意保持很小：只保存 `snapshot_id`、抓取时间、读取边界、UI 观察结果的
SHA-256 hash、前台 app line 的 hash，以及可用时的前台 pid。它不会持久化原始 Accessibility tree、
app 名称、元素名称、字段值或窗口文本。

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

第一个里程碑基本是只读能力，并把少量写路径限制在经过 approval 的 observed ref 动作上。它让 agent
知道原生桌面自动化是否可用，并得到一个可审计的桌面 UI tree。后续如果加入更多写操作，应该继续由
这些边界保护：

- 明确的工具名和参数；
- 写操作前先只读 inspect ref，确认系统报告的可用原生 action；
- 写操作前先只读 find，定位 observed ref；
- 请求写动作前先只读等待 ref 就绪；
- 对关键目标使用写动作前 ref guard，检查 role、文本和状态；
- 写动作前检查前台 app 名称或 pid；
- 小范围原生 action 白名单，而不是任意 AX action 执行；
- `perform_action` 执行前先检查 native action 仍然可用；
- 动作之后先只读等待，再选择下一个 ref；
- 绑定 snapshot 的 refs，而不是猜测坐标；
- 小步、绑定 ref 的 scroll，而不是全局滚轮注入；
- 按键导航前先聚焦可观察的 UI 目标；
- 窄按键白名单，而不是任意键盘注入；
- 本地 `tool_policy` approval 规则；
- 已脱敏的 event 和 archive 记录；
- 桌面 timeline 中可见的执行事件；
- 每个平台执行前的权限检查。

这样 computer use 才和 Crab 的 agent-loop 理念一致：模型可以理解桌面，但 runtime 仍然负责权限、
证据和行动边界。
