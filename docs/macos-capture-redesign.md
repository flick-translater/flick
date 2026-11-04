# macOS 截图交互重构方案

## 问题结论

当前 Flick 的截图交互建立在 `Tauri WebviewWindow` 透明置顶窗口之上，并且在截图会话中会显式调用：

- `window.set_focus()`
- `focus_capture_window`
- `window.addEventListener('focus' | 'mouseenter', ...)`

这会让 Flick 成为 frontmost app。直接后果就是：

- 菜单栏切换为 Flick
- 目标应用的下拉菜单 / popover / context menu 会失去激活状态并关闭
- 鼠标悬停只能命中 Flick 的 overlay，无法命中真实窗口层级
- 现在的“自动蓝框”本质上只能基于显示器 overlay，而不是基于系统真实窗口

这不是参数问题，而是实现模型选错了。

## 新的实现原则

截图交互要拆成两层：

1. `显示层`
   - 使用 macOS 原生 `NSPanel` / `NSWindow`
   - 必须是 `non-activating`
   - 只负责绘制遮罩、蓝框、十字线
   - 不让 Flick 成为 active app

2. `命中层`
   - 不依赖 overlay 接收 hover 结果
   - 直接用系统 API 枚举当前鼠标下的窗口 / 菜单 / popup
   - 根据真实系统元素计算高亮框

3. `捕获层`
   - 用户点击后先隐藏显示层
   - 再用 Core Graphics 对目标矩形截图
   - 整个过程中尽量不切换前台应用

## 推荐架构

### 1. 放弃 Webview capture overlay 作为主交互层

`capture.html` 这套方案可以保留给非 macOS，macOS 不应继续依赖它做鼠标交互。

macOS 上应新增一个原生 capture controller，负责：

- 创建每个屏幕一个 non-activating overlay panel
- 用原生 `NSTrackingArea` 或全局鼠标监听刷新 hover
- 根据系统窗口命中结果绘制高亮框
- 鼠标按下/拖拽/抬起时驱动区域选择
- 截图前临时 `orderOut`

### 2. overlay 必须是 non-activating panel

关键点不是“置顶”，而是“置顶但不激活应用”。

建议属性：

- `NSPanel`
- `NSWindowStyleMaskBorderless`
- `NSWindowStyleMaskNonactivatingPanel`
- `level = CGWindowLevelForKey(.screenSaverWindow)` 或接近的高层级
- `collectionBehavior` 包含：
  - `canJoinAllSpaces`
  - `fullScreenAuxiliary`
  - `ignoresCycle`
- `becomesKeyOnlyIfNeeded = false`
- 展示时使用 `orderFrontRegardless`，不要 `makeKeyAndOrderFront`

这样菜单栏通常会保持在原应用，不会切到 Flick。

### 3. hover 高亮不要靠 overlay 命中，要靠系统窗口枚举

推荐两级命中策略：

1. 首先使用 `CGWindowListCopyWindowInfo`
   - 取得屏幕上可见窗口列表、层级、bounds、owner、window id
   - 按 z-order 从前到后找到鼠标点下的最前窗口

2. 对特殊 UI 元素补充 Accessibility 命中
   - 使用 `AXUIElementCopyElementAtPosition`
   - 目标是命中：
     - 下拉菜单
     - 弹出菜单
     - 某些非标准 panel / sheet
   - 再通过 `AXFrame` 或父级窗口关系推导高亮矩形

原因：

- 纯 `CGWindowListCopyWindowInfo` 对大多数普通窗口够用
- 菜单、状态栏弹层、系统 popup 往往更适合用 AX 命中
- 这也是你提到“别的软件连下拉菜单也能选中”的关键

### 4. 点击模式分两种

建议显式支持两种模式：

1. `智能窗口模式`
   - 默认模式
   - 鼠标移动时自动高亮当前最前真实窗口/菜单
   - 单击直接截取该矩形

2. `自由框选模式`
   - 按下并拖拽时进入
   - 忽略自动窗口命中
   - 用鼠标拖出任意矩形

这样可以兼顾你现在的框选能力和更接近系统工具的窗口捕获体验。

## 现有代码里必须替换的部分

### 必须废弃的交互假设

- `src-tauri/src/features/capture/session.rs` 中的 `open_capture_overlay`
- `src-tauri/src/features/capture/session.rs` 中的 `focus_capture_window`
- `frontend/src/capture.ts` 中所有依赖：
  - `focus`
  - `mouseenter`
  - `mousemove`
  - `mousedown`
  - `mouseup`
  - `get_global_cursor_position + focus_capture_window`

这些逻辑都建立在 “overlay 是当前活跃窗口” 的前提上，无法满足目标体验。

### 还能保留的部分

- `services/screen_capture/macos.rs`
  - 截图底层仍可继续使用 Core Graphics
- 截图后的保存、剪贴板、OCR、翻译链路
- 历史记录与文件落盘逻辑

## 第三方库建议

没有一个现成 Rust crate 可以直接把这整套 macOS 截图交互完整做完。这里更适合：

- 截图和窗口枚举使用系统 API
- 少量借助现成 crate
- 交互层自己写一个小型原生 bridge

### 值得用的库

1. `core-graphics`
   - 已经在用
   - 适合截图、窗口列表、坐标系统转换

2. `objc2` + `objc2-app-kit`
   - 已经在用
   - 适合自己创建 `NSPanel`、处理窗口级别、collection behavior

3. `accessibility-sys` 或自己补 FFI
   - 用于 `AXUIElementCopyElementAtPosition`
   - 这是命中菜单 / popup 的关键

### 不建议依赖的方向

1. 继续找 Tauri 插件解决 overlay
   - Tauri 对 macOS non-activating capture overlay 不是现成能力
   - 即使能建透明窗口，也绕不开 frontmost/focus 问题

2. 继续用纯前端 Webview 捕获 hover
   - 只能命中 Flick 自己的窗口
   - 无法知道真实最上层菜单/窗口

3. 依赖通用 screenshot crate 解决交互
   - 大多数 crate 只解决“截什么”
   - 不解决“如何无激活地选择什么”

## 落地步骤

1. 新增 `macos_capture_overlay` 原生模块
   - Rust + `objc2-app-kit`
   - 负责创建/销毁 non-activating panels

2. 新增 `macos_window_query` 模块
   - `CGWindowListCopyWindowInfo`
   - 返回当前可选窗口列表与 z-order

3. 新增 `macos_accessibility` 模块
   - `AXUIElementCopyElementAtPosition`
   - 专门处理菜单、popover、特殊控件命中

4. 新增统一命中逻辑
   - `hit_test(point) -> HoverTarget`
   - `HoverTarget` 支持：
     - `Window`
     - `Menu`
     - `Panel`
     - `Freeform`

5. 将 macOS 的 capture session 改为原生事件驱动
   - 前端不再负责 capture overlay 交互
   - 前端只接收最终截图完成事件

6. 保留非 macOS 的现有 Webview overlay 实现

## 实际判断

如果目标是“像 CleanShot / Shottr 那样”：

- 继续修补现在的 Tauri Webview overlay，收益很低
- 正确方向是 macOS 下单独实现一层原生非激活截图交互

也就是说，这不是一个 CSS、z-index、`always_on_top` 或 `focusable` 的问题，而是需要把 macOS 截图交互从 Webview 模型切到原生模型。
