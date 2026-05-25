// src/gui/capture_overlay.rs
//
// 捕获引导覆盖层窗口
// 使用纯 Win32 API（替代 egui::show_viewport_immediate）

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT};
use windows::Win32::Foundation::HINSTANCE;
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, EndPaint, FillRect, HBRUSH,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow,
    GetCursorPos, GetSystemMetrics, MoveWindow,
    RegisterClassExW, SetLayeredWindowAttributes,
    ShowWindow,
    SM_CXSCREEN, SM_CYSCREEN, SW_SHOWNA,
    WNDCLASSEXW, CS_HREDRAW, CS_VREDRAW, LWA_ALPHA,
    WM_CLOSE, WM_DESTROY, WM_PAINT,
    WS_EX_LAYERED, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE;
use windows::core::{PCWSTR, w};

const OVERLAY_WIDTH: i32 = 280;
const OVERLAY_HEIGHT: i32 = 70;
const MARGIN: i32 = 12;

/// Store HWND as u64 for Send safety
static OVERLAY_HWND: AtomicU64 = AtomicU64::new(0);
static OVERLAY_VISIBLE: AtomicBool = AtomicBool::new(false);
static WINDOW_CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);

fn set_overlay_hwnd(hwnd: Option<HWND>) {
    let ptr = match hwnd {
        Some(h) => h.0 as u64,
        None => 0,
    };
    OVERLAY_HWND.store(ptr, Ordering::SeqCst);
}

fn get_overlay_hwnd() -> Option<HWND> {
    let ptr = OVERLAY_HWND.load(Ordering::SeqCst);
    if ptr == 0 { None } else { Some(HWND(ptr as *mut _)) }
}

const CLASS_NAME: PCWSTR = w!("ElementSelectorCaptureOverlay");

fn register_overlay_class() {
    if WINDOW_CLASS_REGISTERED.load(Ordering::SeqCst) {
        return;
    }

    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(overlay_wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: unsafe {
            let hmod = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default();
            HINSTANCE(hmod.0 as _)
        },
        hIcon: unsafe { windows::Win32::UI::WindowsAndMessaging::LoadIconW(None, windows::Win32::UI::WindowsAndMessaging::IDI_APPLICATION) }.unwrap_or_else(|_| unsafe { windows::Win32::UI::WindowsAndMessaging::LoadIconW(None, windows::Win32::UI::WindowsAndMessaging::IDI_APPLICATION).unwrap_or_default() }),
        hCursor: unsafe { windows::Win32::UI::WindowsAndMessaging::LoadCursorW(None, windows::Win32::UI::WindowsAndMessaging::IDC_ARROW) }.unwrap_or_default(),
        hbrBackground: HBRUSH(0 as _),
        lpszMenuName: PCWSTR::null(),
        lpszClassName: CLASS_NAME,
        hIconSm: unsafe { windows::Win32::UI::WindowsAndMessaging::LoadIconW(None, windows::Win32::UI::WindowsAndMessaging::IDI_APPLICATION) }.unwrap_or_else(|_| unsafe { windows::Win32::UI::WindowsAndMessaging::LoadIconW(None, windows::Win32::UI::WindowsAndMessaging::IDI_APPLICATION).unwrap_or_default() }),
    };

    unsafe {
        RegisterClassExW(&wc);
    }
    WINDOW_CLASS_REGISTERED.store(true, Ordering::SeqCst);
}

unsafe extern "system" fn overlay_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = windows::Win32::Graphics::Gdi::PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);

            // Simple background fill
            let brush = CreateSolidBrush(COLORREF(0x00F0F0F0));
            let rect = RECT {
                left: 0, top: 0,
                right: OVERLAY_WIDTH,
                bottom: OVERLAY_HEIGHT,
            };
            let _ = FillRect(hdc, &rect, brush);

            // Single border
            let border_brush = CreateSolidBrush(COLORREF(0x00999999));
            let _ = FillRect(hdc, &RECT { left: 0, top: 0, right: 1, bottom: OVERLAY_HEIGHT }, border_brush);
            let _ = FillRect(hdc, &RECT { left: OVERLAY_WIDTH - 1, top: 0, right: OVERLAY_WIDTH, bottom: OVERLAY_HEIGHT }, border_brush);
            let _ = FillRect(hdc, &RECT { left: 0, top: 0, right: OVERLAY_WIDTH, bottom: 1 }, border_brush);
            let _ = FillRect(hdc, &RECT { left: 0, top: OVERLAY_HEIGHT - 1, right: OVERLAY_WIDTH, bottom: OVERLAY_HEIGHT }, border_brush);

            // Draw text with system font
            use windows::Win32::Graphics::Gdi::{
                SetBkMode, SetTextColor, TextOutW, TRANSPARENT,
            };
            let _ = SetBkMode(hdc, TRANSPARENT);
            let _ = SetTextColor(hdc, COLORREF(0x00222222));

            let lines = [
                "捕获模式 (Ctrl+点击)",
                "左键: 选中  |  右键: 多选",
                "右键双击: 退出  |  中键: 切换",
            ];
            let text_y_start = 8;
            let line_height = 18;
            for (i, line) in lines.iter().enumerate() {
                let w: Vec<u16> = line.encode_utf16().collect();
                let _ = TextOutW(hdc, 10, text_y_start + i as i32 * line_height, &w);
            }

            let _ = EndPaint(hwnd, &ps);
            windows::Win32::Foundation::LRESULT(0)
        }
        WM_CLOSE | WM_DESTROY => {
            let _ = DestroyWindow(hwnd);
            windows::Win32::Foundation::LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub struct CaptureOverlay {
    visible: bool,
    logged_screen: bool,
}

impl CaptureOverlay {
    pub fn new() -> Self {
        Self {
            visible: false,
            logged_screen: false,
        }
    }

    pub fn show(&mut self) {
        self.visible = true;
        self.logged_screen = false;

        register_overlay_class();

        let pos = self.smart_position();

        unsafe {
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TRANSPARENT,
                CLASS_NAME,
                w!("捕获"),
                WINDOW_STYLE(WS_POPUP.0),
                pos.0, pos.1,
                OVERLAY_WIDTH,
                OVERLAY_HEIGHT,
                None,
                None,
                Some(windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default().into()),
                None,
            );

            if hwnd.is_err() {
                log::error!("Failed to create overlay window");
                return;
            }

            let hwnd = hwnd.unwrap();

            // Make transparent (layered)
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 200, LWA_ALPHA);

            // Show without activating
            let _ = ShowWindow(hwnd, SW_SHOWNA);

            set_overlay_hwnd(Some(hwnd));
        }

        OVERLAY_VISIBLE.store(true, Ordering::SeqCst);
    }

    pub fn hide(&mut self) {
        self.visible = false;

        if let Some(hwnd) = get_overlay_hwnd() {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
        }
        set_overlay_hwnd(None);
        OVERLAY_VISIBLE.store(false, Ordering::SeqCst);
    }

    fn smart_position(&mut self) -> (i32, i32) {
        let sw = unsafe { GetSystemMetrics(SM_CXSCREEN) };
        let sh = unsafe { GetSystemMetrics(SM_CYSCREEN) };

        if !self.logged_screen {
            self.logged_screen = true;
            let mx = Self::mouse_x();
            log::info!("[overlay] screen: {}x{}, mouse_x: {:?}", sw, sh, mx);
        }

        let x = match Self::mouse_x() {
            Some(mx) if mx < sw / 2 => sw - OVERLAY_WIDTH - MARGIN,
            Some(_) => MARGIN,
            None => sw - OVERLAY_WIDTH - MARGIN,
        };

        (x, MARGIN)
    }

    fn mouse_x() -> Option<i32> {
        unsafe {
            let mut pt = POINT::default();
            if GetCursorPos(&mut pt).is_ok() {
                Some(pt.x)
            } else {
                None
            }
        }
    }

    pub fn update_position(&mut self) {
        if let Some(hwnd) = get_overlay_hwnd() {
            let (x, y) = self.smart_position();
            unsafe {
                let _ = MoveWindow(hwnd, x, y, OVERLAY_WIDTH, OVERLAY_HEIGHT, true.into());
            }
        }
    }
}

impl Default for CaptureOverlay {
    fn default() -> Self {
        Self::new()
    }
}
