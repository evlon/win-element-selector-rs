// src/gui/input_hook.rs
//
// 全局鼠标钩子：捕获时才注册，捕获结束立即卸载。
// 使用 SetWindowsHookExW / UnhookWindowsHookExW 替代 rdev::grab，
// 解决调试时鼠标卡顿问题（rdev::grab 不支持卸载，钩子常驻导致断点时鼠标冻结）。

use crossbeam_channel::{Sender, Receiver, unbounded};
use log::{debug, error, info};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicPtr, Ordering};
use std::thread;

use windows::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM, POINT},
    UI::WindowsAndMessaging::{
        SetWindowsHookExW, UnhookWindowsHookEx, CallNextHookEx,
        GetMessageW, PostThreadMessageW,
        WH_MOUSE_LL, WM_LBUTTONDOWN, WM_LBUTTONUP,
        WM_RBUTTONDOWN, WM_RBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP,
        WM_QUIT, MSG, HHOOK, WINDOWS_HOOK_ID,
        GetCursorPos,
    },
    System::Threading::GetCurrentThreadId,
};

// ─── 公开类型 ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseEvent {
    LeftClick(i32, i32),
    RightClick(i32, i32),
    MiddleClick(i32, i32),
    RightDoubleClick,
}

// ─── 内部状态 ────────────────────────────────────────────────────────────────

static CLICK_CHANNEL: once_cell::sync::Lazy<(Sender<MouseEvent>, Mutex<Option<Receiver<MouseEvent>>>)> =
    once_cell::sync::Lazy::new(|| {
        let (tx, rx) = unbounded();
        (tx, Mutex::new(Some(rx)))
    });

static MOUSE_STATE: once_cell::sync::Lazy<Mutex<(i32, i32, u64)>> =
    once_cell::sync::Lazy::new(|| Mutex::new((0, 0, 0)));

/// 当前钩子句柄，null = 未安装
static HOOK_HANDLE: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

/// 钩子线程 ID（用于 PostThreadMessage WM_QUIT 通知线程退出）
static HOOK_THREAD_ID: AtomicU64 = AtomicU64::new(0);

/// 捕获是否激活（回调中读取，无需锁）
static CAPTURE_ACTIVE: AtomicBool = AtomicBool::new(false);

/// 吞噬标志：钩子回调设置，回调内清除
static SWALLOW_LEFT: AtomicBool = AtomicBool::new(false);
static SWALLOW_RIGHT: AtomicBool = AtomicBool::new(false);
static SWALLOW_MIDDLE: AtomicBool = AtomicBool::new(false);

/// 右键双击检测
static LAST_RIGHT_CLICK_TIME: AtomicU64 = AtomicU64::new(0);

// ─── 辅助函数 ────────────────────────────────────────────────────────────────

fn is_ctrl_pressed() -> bool {
    unsafe {
        let state = windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(17); // VK_CONTROL
        (state as u16) & 0x8000 != 0
    }
}

fn detect_right_double_click() -> bool {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    let last = LAST_RIGHT_CLICK_TIME.load(Ordering::SeqCst);
    LAST_RIGHT_CLICK_TIME.store(now, Ordering::SeqCst);

    now - last < 500 && last > 0
}

// ─── 钩子回调 ────────────────────────────────────────────────────────────────

unsafe extern "system" fn mouse_hook_callback(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code < 0 {
        // MSDN: 如果 code < 0，直接 CallNextHookEx 不处理
        return CallNextHookEx(None, code, wparam, lparam);
    }

    let msll = &*(lparam.0 as *const windows::Win32::UI::WindowsAndMessaging::MSLLHOOKSTRUCT);
    let x = msll.pt.x;
    let y = msll.pt.y;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    // 始终更新鼠标位置
    *MOUSE_STATE.lock() = (x, y, now_ms);

    let msg = wparam.0 as u32;

    match msg {
        WM_LBUTTONDOWN => {
            if CAPTURE_ACTIVE.load(Ordering::SeqCst) && is_ctrl_pressed() {
                info!("Ctrl+Left click at ({}, {})", x, y);
                let _ = CLICK_CHANNEL.0.send(MouseEvent::LeftClick(x, y));
                SWALLOW_LEFT.store(true, Ordering::SeqCst);
                return LRESULT(1); // 吞噬
            }
        }
        WM_LBUTTONUP => {
            if SWALLOW_LEFT.swap(false, Ordering::SeqCst) {
                return LRESULT(1);
            }
        }
        WM_RBUTTONDOWN => {
            if CAPTURE_ACTIVE.load(Ordering::SeqCst) && is_ctrl_pressed() {
                if detect_right_double_click() {
                    info!("Ctrl+Right double click - exit capture mode");
                    let _ = CLICK_CHANNEL.0.send(MouseEvent::RightDoubleClick);
                } else {
                    info!("Ctrl+Right click at ({}, {})", x, y);
                    let _ = CLICK_CHANNEL.0.send(MouseEvent::RightClick(x, y));
                }
                SWALLOW_RIGHT.store(true, Ordering::SeqCst);
                return LRESULT(1);
            }
        }
        WM_RBUTTONUP => {
            if SWALLOW_RIGHT.swap(false, Ordering::SeqCst) {
                return LRESULT(1);
            }
        }
        WM_MBUTTONDOWN => {
            if CAPTURE_ACTIVE.load(Ordering::SeqCst) && is_ctrl_pressed() {
                info!("Ctrl+Middle click at ({}, {})", x, y);
                let _ = CLICK_CHANNEL.0.send(MouseEvent::MiddleClick(x, y));
                SWALLOW_MIDDLE.store(true, Ordering::SeqCst);
                return LRESULT(1);
            }
        }
        WM_MBUTTONUP => {
            if SWALLOW_MIDDLE.swap(false, Ordering::SeqCst) {
                return LRESULT(1);
            }
        }
        _ => {}
    }

    // 不吞噬：传递给下一个钩子
    CallNextHookEx(None, code, wparam, lparam)
}

// ─── 公开 API ────────────────────────────────────────────────────────────────

/// 初始化通道（不再启动钩子线程，钩子在 activate_capture 时按需安装）
pub fn init() -> anyhow::Result<()> {
    // 只需确保通道存在（Lazy 已处理）
    info!("Input hook system initialized (lazy — hook installs on capture)");
    Ok(())
}

/// 激活捕获：安装 WH_MOUSE_LL 钩子
pub fn activate_capture() {
    CAPTURE_ACTIVE.store(true, Ordering::SeqCst);
    LAST_RIGHT_CLICK_TIME.store(0, Ordering::SeqCst);

    // 如果钩子已安装，不需要重复安装
    if !HOOK_HANDLE.load(Ordering::SeqCst).is_null() {
        info!("Capture activated (hook already installed)");
        return;
    }

    thread::spawn(move || {
        let thread_id = unsafe { GetCurrentThreadId() };
        HOOK_THREAD_ID.store(thread_id as u64, Ordering::SeqCst);

        let hook = unsafe {
            SetWindowsHookExW(
                WINDOWS_HOOK_ID(WH_MOUSE_LL.0),
                Some(mouse_hook_callback as unsafe extern "system" fn(i32, WPARAM, LPARAM) -> LRESULT),
                None,
                0,
            )
        };

        match hook {
            Ok(h) => {
                HOOK_HANDLE.store(h.0, Ordering::SeqCst);
                info!("WH_MOUSE_LL hook installed (thread {})", thread_id);

                // 消息循环：保持钩子线程存活
                let mut msg = MSG::default();
                loop {
                    let ret = unsafe { GetMessageW(&mut msg, None, 0, 0) };
                    if ret.0 <= 0 || msg.message == WM_QUIT {
                        break;
                    }
                }

                // 卸载钩子
                unsafe {
                    let h = HOOK_HANDLE.swap(std::ptr::null_mut(), Ordering::SeqCst);
                    if !h.is_null() {
                        let _ = UnhookWindowsHookEx(HHOOK(h));
                    }
                }
                info!("WH_MOUSE_LL hook uninstalled (thread {})", thread_id);
            }
            Err(e) => {
                error!("Failed to install WH_MOUSE_LL hook: {:?}", e);
                HOOK_HANDLE.store(std::ptr::null_mut(), Ordering::SeqCst);
            }
        }

        HOOK_THREAD_ID.store(0, Ordering::SeqCst);
    });

    // 等钩子安装完成（短暂等待）
    for _ in 0..20 {
        if !HOOK_HANDLE.load(Ordering::SeqCst).is_null() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    info!("Capture activated");
}

/// 停止捕获：卸载钩子
pub fn deactivate_capture() {
    CAPTURE_ACTIVE.store(false, Ordering::SeqCst);

    // 重置吞噬标志
    SWALLOW_LEFT.store(false, Ordering::SeqCst);
    SWALLOW_RIGHT.store(false, Ordering::SeqCst);
    SWALLOW_MIDDLE.store(false, Ordering::SeqCst);
    LAST_RIGHT_CLICK_TIME.store(0, Ordering::SeqCst);

    // 通知钩子线程退出
    let thread_id = HOOK_THREAD_ID.load(Ordering::SeqCst);
    if thread_id != 0 {
        unsafe {
            let _ = PostThreadMessageW(
                thread_id as u32,
                WM_QUIT,
                WPARAM(0),
                LPARAM(0),
            );
        }
        HOOK_THREAD_ID.store(0, Ordering::SeqCst);
    }

    debug!("Capture deactivated (hook uninstall requested)");
}

#[allow(dead_code)]
pub fn is_active() -> bool {
    CAPTURE_ACTIVE.load(Ordering::SeqCst)
}

pub fn poll_mouse_click() -> Option<MouseEvent> {
    CLICK_CHANNEL.1.lock().as_ref().and_then(|rx| rx.try_recv().ok())
}

pub fn get_mouse_state() -> (i32, i32, u64) {
    let (x, y, t) = *MOUSE_STATE.lock();
    // 钩子未安装时 MOUSE_STATE 无数据，用 GetCursorPos 后备
    if x == 0 && y == 0 {
        let mut pt = POINT::default();
        unsafe {
            if GetCursorPos(&mut pt).is_ok() {
                return (pt.x, pt.y, t);
            }
        }
    }
    (x, y, t)
}

#[allow(dead_code)]
pub fn cleanup() {
    deactivate_capture();
    *CLICK_CHANNEL.1.lock() = None;
    info!("Input hook system cleaned up");
}
