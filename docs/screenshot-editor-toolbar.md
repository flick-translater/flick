# 截图编辑工具栏功能方案

## 功能目标

普通截图完成后，在保存前提供一个轻量图片编辑工具栏。用户可以对截图进行标注、遮挡和文字说明，点击确认后：

- 保存编辑后的图片到当前截图目录
- 将编辑后的图片写入系统剪贴板，用户可以直接粘贴
- 写入截图历史，历史记录中的图片必须是编辑后的最终版本

该工具栏只作用于普通截图，不作用于截图并翻译。截图并翻译必须保持当前流程：截图完成后直接保存、复制、OCR、翻译，不打开编辑工具栏。

该工具栏由设置项控制，默认开启。关闭后普通截图保持当前行为：截图完成后直接保存并复制原图。

## 用户流程

### 普通截图

1. 用户通过截图快捷键进入截图选择。
2. 用户选择区域并完成截图。
3. 如果 `截图编辑工具栏` 开启：
   - 打开截图编辑窗口 / 浮层
   - 展示刚截取的图片
   - 工具栏默认选中画笔或上一次使用的工具
4. 用户完成编辑后点击确认。
5. 应用保存编辑后的图片、复制到剪贴板、更新截图历史。

### 截图并翻译

截图并翻译不进入编辑工具栏，不受 `截图编辑工具栏` 设置影响。

流程保持现状：

1. 用户通过截图并翻译快捷键进入截图选择。
2. 用户选择区域并完成截图。
3. 应用直接保存原图、复制到剪贴板、更新截图历史。
4. 应用继续运行 OCR 和翻译，并展示翻译窗口。

也就是说，本功能不能改变截图并翻译的等待时机、OCR 输入图、历史记录内容和剪贴板内容。

## 工具栏能力

### 通用控件

工具栏建议提供：

- 工具选择：画笔、直线、箭头、圆、框、马赛克、文字
- 颜色选择：至少提供红、黄、蓝、绿、白、黑
- 粗细选择：标注工具的线宽
- 撤销 / 重做
- 清空所有标注
- 取消
- 确认

颜色和粗细对画笔、直线、箭头、圆、框、文字生效。马赛克使用独立的块大小或强度参数。

### 画笔

- 鼠标按下开始路径，拖动追加路径点，抬起结束。
- 默认圆角线帽和圆角连接。
- 支持颜色和线宽。

### 直线

- 鼠标按下记录起点，拖动时预览，抬起生成线段。
- 支持颜色和线宽。
- 按住 Shift 时可吸附到水平、垂直和 45 度方向。

### 箭头

- 行为与直线一致。
- 在线段终点绘制箭头。
- 箭头大小随线宽变化，避免细线箭头过大或粗线箭头过小。

### 圆

- 鼠标拖拽生成椭圆外接矩形。
- 默认无填充，仅描边。
- 支持颜色和线宽。
- 按住 Shift 时约束为正圆。

### 框

- 鼠标拖拽生成矩形。
- 默认无填充，仅描边。
- 支持颜色和线宽。
- 按住 Shift 时约束为正方形。

### 马赛克

- 鼠标拖拽生成马赛克区域。
- 对确认后的最终图片执行像素化，不仅是视觉遮罩。
- 建议实现为矩形区域马赛克，后续再扩展画笔式马赛克。
- 马赛克块大小建议默认 10-16px，并提供强度选择。

### 插入文字

- 点击图片创建文字框。
- 支持输入、移动、确认文本。
- 支持颜色和字号。
- 空文本框失焦后自动移除。
- 文字编辑状态下，Esc 取消当前文本框，Enter 确认输入；多行文本可用 Shift+Enter。

## 编辑器交互

### 画布

编辑器应以截图原始尺寸作为逻辑坐标系。界面中可以缩放显示，但所有标注数据必须存储为原图坐标，最终导出时按原图尺寸渲染，避免高分屏或缩放窗口导致标注错位。

建议数据结构：

```ts
type ScreenshotAnnotation =
  | { kind: 'pen'; points: Point[]; color: string; width: number }
  | { kind: 'line'; from: Point; to: Point; color: string; width: number }
  | { kind: 'arrow'; from: Point; to: Point; color: string; width: number }
  | { kind: 'ellipse'; rect: Rect; color: string; width: number }
  | { kind: 'rect'; rect: Rect; color: string; width: number }
  | { kind: 'mosaic'; rect: Rect; blockSize: number }
  | { kind: 'text'; position: Point; text: string; color: string; fontSize: number };
```

### 确认与取消

- 确认：导出最终 PNG，进入普通截图的保存、剪贴板、历史链路。
- 取消：丢弃截图，不保存、不复制、不写历史。
- 关闭窗口：等同取消。
- Esc：如果正在编辑文字，先退出文字编辑；否则取消整个编辑。

### 快捷键

- `Ctrl+Z`：撤销上一步编辑操作。
- macOS 同时支持 `Command+Z` 撤销，符合系统习惯。
- `Ctrl+Shift+Z` 和 `Ctrl+Y`：重做上一步撤销操作。
- macOS 同时支持 `Command+Shift+Z` 重做。
- 撤销 / 重做应覆盖画笔、直线、箭头、圆、框、马赛克、文字新增、文字修改和清空标注。
- 文字输入框聚焦时，`Ctrl+Z` / `Command+Z` 优先撤销文字输入内容；如果输入框没有可撤销内容，再交给编辑器撤销栈。

### 工具栏位置

工具栏应贴近图片编辑区域，但不能遮挡确认按钮。推荐：

- 桌面端：顶部横向工具栏，确认 / 取消放在右侧。
- 小窗口或窄屏：工具栏允许换行或折叠到两行。

工具按钮使用图标为主，悬停显示 tooltip。不要只依赖文字按钮，否则工具栏会过宽。

## 与现有截图链路的关系

当前截图完成后，`src-tauri/src/features/capture/session.rs` 会直接：

- `save_png`
- `copy_to_clipboard`
- 更新截图历史
- 发出 `capture-finished`
- 在截图并翻译流程中运行 OCR 和翻译

新增编辑器后，这段逻辑需要拆成两步：

1. `截图阶段`
   - 只负责得到原始 `ImageBuffer`
   - 如果是截图并翻译，始终沿用当前直接保存、剪贴板、OCR、翻译逻辑
   - 如果是普通截图且设置关闭，沿用当前直接保存逻辑
   - 如果是普通截图且设置开启，把图片交给编辑器

2. `提交阶段`
   - 接收编辑器导出的最终图片
   - 只用于普通截图
   - 统一调用普通截图的保存、剪贴板、历史、事件发送逻辑

建议抽出普通截图最终提交函数，避免“直接保存原图”和“编辑后保存”各自维护一套保存逻辑：

```rust
fn finalize_regular_capture_image(
    app: &AppHandle,
    state: &AppState,
    image: ImageBuffer<Rgba<u8>, Vec<u8>>,
) -> Result<CaptureRecord, FlickError>
```

实际签名可以根据当前模块所有权调整，但目标是让普通截图的“原图直接保存”和“编辑后保存”复用同一条最终提交链路。截图并翻译流程不应该依赖该编辑提交函数，除非只是复用底层保存、剪贴板、历史等更小粒度的工具函数。

## 设置项

### 数据模型

在 `AppSettings` 中新增字段：

```rust
pub screenshot_editor_toolbar_enabled: bool,
```

默认值：

```rust
screenshot_editor_toolbar_enabled: true,
```

由于 `AppSettings` 已使用 `#[serde(default)]`，老用户升级后缺失字段时应自动回落到默认开启。

前端 `AppSettings` 类型同步增加：

```ts
screenshot_editor_toolbar_enabled: boolean;
```

### Tauri 命令

新增设置命令：

```rust
#[tauri::command]
pub fn update_screenshot_editor_toolbar_enabled(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<AppSettings, FlickError>
```

命令行为：

- 更新内存中的 `state.settings`
- 调用 `settings_store.save_settings`
- 返回更新后的完整 `AppSettings`

### 设置界面

在通用设置页增加一个开关：

- 中文：截图后显示编辑工具栏
- 英文：Show editor toolbar after screenshot
- 日文：スクリーンショット後に編集ツールバーを表示

建议放在截图相关设置附近，例如截图目录和截图快捷键之间。

默认开启。该开关仅控制普通截图。关闭后，普通截图体验应与现有版本一致；截图并翻译无论开关开启或关闭都保持现有流程。

## 前端实现建议

新增编辑器模块：

- `frontend/src/views/ScreenshotEditor.tsx` 或独立入口 `frontend/src/screenshot-editor.tsx`
- `frontend/src/components/screenshot-editor/Toolbar.tsx`
- `frontend/src/components/screenshot-editor/Canvas.tsx`
- `frontend/src/components/screenshot-editor/types.ts`

渲染策略：

- 底图使用 `<canvas>` 或 `<img>` 展示。
- 标注层使用 canvas 绘制，便于最终导出 PNG。
- 文字输入可以临时使用 HTML input/textarea 覆盖在画布上，确认后转为 annotation。
- 导出时使用离屏 canvas：先绘制原图，再按顺序绘制 annotation，最后导出 PNG bytes。

编辑器向后端提交时建议传 PNG bytes 或临时文件路径。优先传 bytes，避免产生未确认的临时文件。

## 后端实现建议

建议新增一个截图编辑会话概念：

- 截图完成后生成 session id
- 后端暂存原始 PNG bytes 或临时文件
- 前端编辑器通过 session id 获取图片
- 用户确认后提交最终 PNG bytes
- 后端调用普通截图最终提交逻辑
- 用户取消时释放 session

这样可以避免把大图片塞进事件 payload，也可以避免未确认图片进入历史目录。

需要新增的命令可以是：

```rust
#[tauri::command]
pub fn get_pending_capture_image(session_id: String) -> Result<String, FlickError>

#[tauri::command]
pub fn confirm_regular_capture_edit(session_id: String, png_bytes: Vec<u8>) -> Result<CaptureRecord, FlickError>

#[tauri::command]
pub fn cancel_capture_edit(session_id: String) -> Result<(), FlickError>
```

如果 Tauri 命令传输 `Vec<u8>` 在目标平台上不稳定，可以改为 base64 字符串，但后端必须限制最大尺寸并返回明确错误。

## 平台要求

确认后的图片复制到剪贴板应复用 `ScreenCaptureService::copy_to_clipboard`，确保 macOS、Windows、Linux 的剪贴板行为一致。

Linux Wayland 环境当前截图能力受限，编辑器不需要额外绕过该限制。只要普通截图能得到图片，就应能进入编辑器。

## 错误处理

- 编辑器打开失败：普通截图回退到直接保存原图，并记录错误日志。
- 图片导出失败：保留编辑器窗口，提示用户重试或取消。
- 保存失败：不应写剪贴板和历史，提示保存失败。
- 剪贴板写入失败：图片仍保存并进入历史，但记录错误日志；当前代码已有类似行为。
- OCR 失败：只属于截图并翻译原有流程，不受本功能影响。

## 验收标准

1. 设置默认开启，老配置升级后也开启。
2. 设置关闭时，普通截图行为与当前版本一致。
3. 普通截图确认编辑后，历史记录展示编辑后的图片。
4. 普通截图确认编辑后，系统剪贴板中是编辑后的图片。
5. 截图并翻译不显示编辑工具栏。
6. 截图并翻译的保存、剪贴板、历史、OCR、翻译流程与当前版本一致。
7. 普通截图取消编辑不会产生历史记录、文件或剪贴板内容。
8. 画笔、直线、箭头、圆、框、马赛克、文字都能导出到最终 PNG。
9. 高分屏和窗口缩放下，标注位置与导出结果一致。
10. 马赛克导出后无法通过隐藏图层恢复原始区域。
11. `Ctrl+Z` 可以撤销上一步编辑操作。
12. 撤销和重做不会破坏文字输入、马赛克和图形标注顺序。

## 测试建议

- 单元测试：annotation 坐标换算、Shift 约束、撤销/重做栈。
- 前端组件测试：工具切换、颜色/线宽变更、文字输入确认和取消、`Ctrl+Z` 撤销。
- 后端测试：设置默认值、设置命令持久化、普通截图编辑确认后 finalize 链路。
- 手工测试：macOS、Windows、Linux 各验证保存、历史和剪贴板。
- 回归测试：截图并翻译在设置开启和关闭时都不出现编辑工具栏，并保持原有 OCR/翻译流程。

## 实施步骤

1. 新增 `screenshot_editor_toolbar_enabled` 设置字段、默认值、Tauri 更新命令和多语言文案。
2. 抽出普通截图最终提交逻辑，让普通截图的直接保存和编辑后保存共享保存、剪贴板、历史和事件发送。
3. 新增编辑会话管理，支持获取待编辑图片、确认、取消。
4. 新增截图编辑窗口 / 浮层和基础 canvas 渲染。
5. 实现画笔、直线、箭头、圆、框。
6. 实现马赛克和文字工具。
7. 接入普通截图流程：设置开启时进入编辑器，关闭时直接 finalize。
8. 补齐测试并进行跨平台手工验证。
