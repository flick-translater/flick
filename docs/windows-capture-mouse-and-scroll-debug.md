# Windows 截图鼠标漂移 & 长截图不滚动 —— 诊断与修复方案

> 适用范围：仅 Windows 平台。macOS 实现已正常，本文不涉及。
> 关联提交：`50958cd feat: unify pipeline logic and improve platform-specific implementations for capture and overlays`
> 日志样本：仓库根目录 `testlog`（UTF-16LE 编码，`iconv -f UTF-16LE -t UTF-8 testlog` 可读）

---

## 0. 现象（用户报告）

1. **普通截图**：打开普通截图时鼠标就会漂移。**未引入长截图功能前没有这个问题。**
2. **长截图**：
   - a) 图片不滚动；
   - b) 鼠标漂移；
   - c) 移动鼠标后停下来，鼠标还会继续往前动一段。
3. 已经尝试过 `SendInput` 和 `PostMessage` 两条路，都没效果。

---

## 1. 结论速览（TL;DR）

| # | 现象 | 根因 | 修复 |
|---|------|------|------|
| 1 | 普通截图 + 长截图鼠标漂移、松手后续动 | 在 `WH_MOUSE_LL` 低级钩子回调里做了**耗时操作**（锁 `Mutex`、写文件日志、跑限速逻辑），触发系统钩子超时，导致鼠标输入管线积压/延迟回放 | 钩子回调瘦身：只读数据 → 写 `Atomic` / 推无锁队列 → 立即 `CallNextHookEx` 返回。所有重活搬到 worker 线程 |
| 2 | 长截图图片不滚动 | 滚轮被 `PostMessage(WM_MOUSEWHEEL)` 投递给 `EnumWindows` 找到的**顶层窗口**；顶层窗口既不滚动也不转发，且 Windows 不支持向非前台窗口模拟输入 | 改用 `SendInput(MOUSEEVENTF_WHEEL)` 注入真实滚轮事件，由系统命中测试投递到光标下正确的子控件 |

两个根因相互独立，但**问题 1 是基础**：钩子回调不瘦身，无论滚动怎么修，漂移都不会好。

---

## 2. 背景：Windows 低级钩子的两条硬规则

修复前必须理解这两条来自微软官方文档的约束，否则改完还会复发。

### 规则 A：低级钩子回调必须立刻返回，不能做重活

`WH_MOUSE_LL` / `WH_KEYBOARD_LL` 的回调有以下特性（[LowLevelMouseProc 官方文档](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelmouseproc)）：

- 回调运行在**安装钩子那个线程的消息循环**上，系统通过给该线程发消息来调用它 —— 所以该线程**必须有消息循环**（`PeekMessage`/`GetMessage`）。
- 系统对回调有**硬超时** `LowLevelHooksTimeout`（注册表项，Win10 1709+ 上限 1000ms，实测体验远低于此）。
- **超时后果**：系统把消息直接传给下一个钩子，并且在 Win7+ 上**静默卸载**这个"卡住"的钩子。更糟的是，在超时边缘徘徊时，整条 raw input → 光标移动 的管线会被拖慢、积压；被延迟的鼠标移动事件随后被批量回放，**用户看到的就是"漂移、松手后还往前动"**。

官方明确的最佳实践原文：

> If the application must use low level hooks, it should **run the hooks on a dedicated thread that passes the work off to a worker thread and then immediately returns.**
> In most cases ... it should monitor **raw input** instead.

即：**回调里只做"读取 + 投递"，所有计算/IO/锁都交给 worker 线程。**

### 规则 B：Windows 不支持向非前台窗口模拟输入

`PostMessage(WM_MOUSEWHEEL)` 把消息塞进目标窗口队列后立即返回 `TRUE`（日志里的 `ok`），但**这只代表"消息已入队"，不代表有人处理**。

- 现代应用（Chrome / Electron / 任何用 raw input、DirectManipulation、或检查 `GetMessageExtraInfo` 的程序）会**忽略**这种合成的后台滚轮消息。
- 微软立场（[The Old New Thing: You can't simulate keyboard input with PostMessage](https://devblogs.microsoft.com/oldnewthing/20250319-00/?p=110979)）：Windows 没有提供向非前台程序模拟输入的受支持途径。
- `WM_MOUSEWHEEL` 还要求投递给**光标正下方那个最深层的子控件**（真正持有滚动条的窗口），投给顶层窗口无效。

可靠的做法是 `SendInput`：它注入的是**真实输入事件**，由系统走正常命中测试投递到光标下的正确窗口，对几乎所有应用都生效。

---

## 3. 逐项诊断（含代码与日志证据）

### 3.1 问题 1：普通截图鼠标漂移

**涉及代码**：`src-tauri/src/features/capture/platform/windows_platform.rs`

钩子回调 `low_level_mouse_proc`（L550-576）每个鼠标事件都执行：

```rust
unsafe extern "system" fn low_level_mouse_proc(code, wparam, lparam) -> LRESULT {
    if code == HC_ACTION as i32 && lparam != 0 {
        // ...
        if let Some((label, down)) = action {
            if let Ok(mut runtime) = native_runtime().lock() {   // ← 在 LL hook 回调里锁 Mutex
                // ...
            }
        }
    }
    CallNextHookEx(null_mut(), code, wparam, lparam)
}
```

同一把 `native_runtime()` Mutex 还被：

- 主轮询循环每 8ms 锁一次：`current_input_state()`（L528）、`take_escape_pressed()`（L535）；
- 安装/卸载钩子、`clear_active_session()` 等多处锁。

**钩子回调线程**与**轮询线程**争抢同一把锁 → 回调偶发阻塞 → 触发规则 A 的超时 → 漂移。

> **为什么"加长截图前没有"**：本次提交重构了普通截图的原生捕获流程，把按键状态集中到 `native_runtime` 这个被多线程高频争用的 Mutex 里。漂移来自这个结构变化，不是长截图代码本身。

### 3.2 问题 2：长截图鼠标漂移 + 续动

**涉及代码**：`src-tauri/src/features/capture/platform/windows_long_capture_scroll_platform.rs`

滚轮钩子回调 `low_level_mouse_proc`（L233）→ `handle_mouse_wheel_event`（L266）在**每个滚轮事件**里做了大量重活：

```rust
fn handle_mouse_wheel_event(state: &mut HookState, info: &MSLLHOOKSTRUCT) -> bool {
    // ...
    long_log(format!(                          // ← 致命：在 LL hook 回调里写文件 I/O
        "scroll_controller/windows: wheel hook pt=({}, {}) ...", ...
    ));
    // ... 限速计算 rate_window.retain / sum / clamp ...
    state.wheel_sender.send(WheelCommand { ... });
}
```

而进入这个函数前，回调已经先 `hook_state().lock()`（L250-258）。

**日志证据**（`testlog`，`wheel hook` 每 ~40-70ms 刷一行，每行都伴随一次文件写入）：

```
[long-capture] ...Z scroll_controller/windows: wheel hook pt=(771, 712) selection=(451,375 662x427) inside=true injected_flags=0 extra=0x0
[long-capture] ...Z scroll_controller/windows: wheel inside selection raw_delta=-120 emit_delta=-42
[long-capture] ...Z scroll_controller/windows: wheel target pt=(771,712) delta=-42 before_hwnd=0x671008 target_hwnd=0x510b50 skipped=2
```

在 LL hook 回调里写文件是规则 A 的最严重违反，直接导致长截图期间的漂移与续动。

### 3.3 问题 3：长截图图片不滚动

**涉及代码**：同上文件，`send_wheel_to_point`（L340）/ `find_scroll_target_window`（L418）/ `post_wheel_message`（L366）。

**日志证据**：

```
PostMessage WM_MOUSEWHEEL ok hwnd=0x510b50 delta=-42
```

- `target_hwnd=0x510b50` 在整个会话里**恒定不变**，它是 `EnumWindows` + `GetWindowRect`（`enum_hit_test_window`, L438）命中测试找出来的**顶层窗口**，不是光标下真正滚动的子控件。
- `PostMessage` 返回 `ok` 只表示入队成功（规则 B），目标应用并不响应。

两点叠加 → 消息发出去了，但没人滚动。

---

## 4. 修复方案

总原则：**钩子回调 = 读取 + 投递 + 立即返回；一切重活在 worker 线程；滚动注入用 `SendInput`。**

### 4.1 修普通截图漂移（windows_platform.rs）

把回调里的 `Mutex` 彻底拿掉，按键/Esc 状态改用原子量：

```rust
use std::sync::atomic::{AtomicBool, Ordering};

static LEFT_DOWN: AtomicBool = AtomicBool::new(false);
static RIGHT_DOWN: AtomicBool = AtomicBool::new(false);
static ESCAPE_PRESSED: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn low_level_mouse_proc(code, wparam, lparam) -> LRESULT {
    if code == HC_ACTION as i32 && lparam != 0 {
        match wparam as u32 {
            WM_LBUTTONDOWN => LEFT_DOWN.store(true, Ordering::Relaxed),
            WM_LBUTTONUP   => LEFT_DOWN.store(false, Ordering::Relaxed),
            WM_RBUTTONDOWN => RIGHT_DOWN.store(true, Ordering::Relaxed),
            WM_RBUTTONUP   => RIGHT_DOWN.store(false, Ordering::Relaxed),
            _ => {}
        }
    }
    CallNextHookEx(null_mut(), code, wparam, lparam)   // 零锁、零 IO，立即返回
}
```

- `current_input_state()` / `take_escape_pressed()` 改为读原子量。
- `native_runtime` 仍可保留 session_id 等非热路径状态，但**绝不在回调里碰它**。
- 键盘回调 `low_level_keyboard_proc` 同样改用 `ESCAPE_PRESSED.store(true, Relaxed)`。

这一步独立、风险低，建议**先单独上线验证**：普通截图漂移应当立刻消失。它也是隔离变量的实验 —— 若漂移消失，即证明根因确为回调锁竞争。

### 4.2 修长截图漂移（windows_long_capture_scroll_platform.rs：钩子瘦身）

回调里**只做三件事**：读 delta、判断是否在选区内、推入无锁队列。然后立即返回。

```rust
// 选区用原子量保存，避免锁
static SEL_X: AtomicI32 = AtomicI32::new(0);
static SEL_Y: AtomicI32 = AtomicI32::new(0);
static SEL_W: AtomicI32 = AtomicI32::new(0);
static SEL_H: AtomicI32 = AtomicI32::new(0);
static STOP: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn low_level_mouse_proc(code, wparam, lparam) -> LRESULT {
    if code == HC_ACTION as i32
        && wparam as u32 == WM_MOUSEWHEEL
        && lparam != 0
        && !STOP.load(Ordering::Relaxed)
    {
        let info = &*(lparam as *const MSLLHOOKSTRUCT);
        // 放行自己 SendInput 注入的事件，防止回环
        if info.flags & LLMHF_INJECTED == 0 && point_in_selection_atomic(info.pt) {
            let raw = ((info.mouseData >> 16) as u16) as i16 as i32;
            if raw != 0 {
                // try_send / SegQueue.push —— 绝不阻塞
                let _ = WHEEL_QUEUE.try_send(WheelCommand { x: info.pt.x, y: info.pt.y, delta: raw });
            }
            return 1;  // 吃掉用户的原始滚轮，由 worker 重新注入受控速度
        }
    }
    CallNextHookEx(null_mut(), code, wparam, lparam)
}
```

强制要求：

- **回调里删除所有 `long_log`**。需要日志只在 worker 线程打。
- 选区命中测试读原子量，不锁 `hook_state()`。
- 队列用 `crossbeam::SegQueue` 或 `mpsc` 的 `try_send`（**绝不 `recv` / `lock`**）。
- 限速逻辑（`rate_window` / `WHEEL_SPEED_FACTOR` 等）整体搬到 worker 线程。

### 4.3 修长截图不滚动（改用 SendInput）

worker 线程消费队列 → 限速 → 用 `SendInput` 注入真实滚轮：

```rust
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_MOUSE, MOUSEINPUT, MOUSEEVENTF_WHEEL, SendInput,
};

const FLICK_INJECT_TAG: usize = 0x464C_4B31; // "FLK1"，配合 dwExtraInfo 识别自己注入的事件

fn inject_wheel(emit_delta: i32) {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: emit_delta as u32, // 带符号的 WHEEL_DELTA 倍数
                dwFlags: MOUSEEVENTF_WHEEL,
                time: 0,
                dwExtraInfo: FLICK_INJECT_TAG,
            },
        },
    };
    unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
}
```

要点（这几条正是之前 "SendInput 试了没效果" 的原因）：

1. **光标位置**：`SendInput(MOUSEEVENTF_WHEEL)` 把滚轮事件投递到**当前光标所在窗口**。用户本来就在选区内滚动，光标已在目标控件上方，所以**不需要也不要再移动光标**。如果之前先 `SetCursorPos` 移到别处再注入，就会投错窗口。
2. **防回环**：注入的事件会再次进入自己的 LL hook。回调里用 `info.flags & LLMHF_INJECTED == 0` 放行（已有），双保险可再比对 `dwExtraInfo == FLICK_INJECT_TAG`。若回调把注入事件也 `return 1` 吃掉，就会"注入即被自己吞"，表现为完全不滚 —— 这是常见坑。
3. **吃掉原始 / 重发受控**：回调对**用户真实滚轮** `return 1`（拦截），worker 再以限速后的 delta 重新 `SendInput`。这样既能限速又能真正滚动。
4. 删掉 `EnumWindows` 命中测试 + `PostMessage` 那一整套（`find_scroll_target_window` / `post_wheel_message` / `send_wheel_to_point` 里的 PostMessage 分支）。

> **备选方案 B（不推荐）**：若坚持 PostMessage，必须用 `WindowFromPoint` 再 `RealChildWindowFromPoint` 递归下钻到最深子窗口，而非 `EnumWindows` 取顶层。但对 Chrome/Electron 仍可能无效，故仅作备案。

---

## 5. 实施顺序与验收

分两步走，**先隔离普通截图漂移，确认根因后再动长截图**：

### 第 1 步：普通截图漂移
- 改动：4.1（windows_platform.rs，回调原子化、去 Mutex）。
- 验收：打开普通截图，移动鼠标 —— 光标跟手、无漂移、无续动；框选、Esc 取消、右键取消均正常。

### 第 2 步：长截图滚动 + 漂移
- 改动：4.2（钩子瘦身）+ 4.3（SendInput 注入）。
- 验收：
  - 在选区内滚动鼠标 → 目标内容**真实滚动**（Chrome / 资源管理器 / 记事本都试）；
  - 滚动期间鼠标**不漂移**，停止后**不续动**；
  - 滚动速度受限速控制、不失控；
  - 工具栏长截图按钮（`start_button_scroll`）同样走 SendInput 路径，行为一致。

---

## 6. 检查清单（防复发）

- [ ] LL hook 回调里**没有** `Mutex::lock()`
- [ ] LL hook 回调里**没有**任何文件 / 日志 IO（`long_log` 等）
- [ ] LL hook 回调里**没有**循环 / 限速 / `EnumWindows` 等耗时调用
- [ ] 回调只用 `Atomic*` 读状态 + 无锁队列 `try_send` 投递 + 立即 `CallNextHookEx`
- [ ] 安装钩子的线程有消息循环（`PeekMessageW` 已具备，L181）
- [ ] 滚动注入用 `SendInput`，不用 `PostMessage` 给顶层窗口
- [ ] 注入事件通过 `LLMHF_INJECTED` / `dwExtraInfo` 被回调放行，无回环
- [ ] 注入前不移动光标（依赖用户光标已在选区内）

---

## 7. 参考资料

- [LowLevelMouseProc callback function — Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelmouseproc)（回调超时与"甩活给 worker 线程"最佳实践、改用 raw input 的建议）
- [SendInput function — Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput)
- [MOUSEINPUT structure — Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-mouseinput)（`MOUSEEVENTF_WHEEL` / `mouseData` / `dwExtraInfo`）
- [The Old New Thing: You can't simulate keyboard input with PostMessage, revisited](https://devblogs.microsoft.com/oldnewthing/20250319-00/?p=110979)（PostMessage 无法可靠模拟输入）
- [Windows Mouse hook lagging — Simone Galleni](https://www.linkedin.com/pulse/windows-mouse-hook-lagging-simone-galleni)（钩子回调拖慢导致鼠标卡顿的案例）
