// src/highlight.rs
//
// 元素高亮显示 - 使用两个独立窗口：
// 1. 标签窗口：显示元素类型，宽度自适应文字
// 2. 高亮框窗口：纯边框，中空透明
//
// 共享模块：供 GUI 和 HTTP API 使用

use crate::core::model::{ElementRect, HighlightInfo};
use log::error;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Arc;

// ─── Public API ──────────────────────────────────────────────────────────────

/// 在元素位置显示高亮闪烁，指定时间后自动消失
pub fn flash(rect: &ElementRect, duration_ms: u64) {
    let info = HighlightInfo::new(rect.clone(), "");
    flash_with_info(&info, duration_ms);
}

/// 在元素位置显示带标签的高亮闪烁，指定时间后自动消失
pub fn flash_with_info(info: &HighlightInfo, duration_ms: u64) {
    windows_impl::flash_with_info(info, duration_ms);
}

/// 在指定坐标显示红色圆点标记（点击留痕），指定时间后自动消失
pub fn flash_point(x: i32, y: i32, duration_ms: u64) {
    point_impl::flash_point(x, y, duration_ms);
}

/// 隐藏当前高亮
pub fn hide() {
    windows_impl::hide();
}

/// 更新高亮显示
pub fn update_highlight(info: &HighlightInfo) {
    windows_impl::update_highlight(info);
}

/// 显示持久高亮（返回 RAII handle，drop 时自动关闭）
pub fn show(rect: &ElementRect) -> HighlightHandle {
    windows_impl::show(rect, "")
}

/// 显示带标签的持久高亮
pub fn show_with_info(info: &HighlightInfo) -> HighlightHandle {
    windows_impl::show(&info.rect, &info.control_type_cn)
}

/// 高亮句柄（RAII），drop 时自动关闭高亮
pub struct HighlightHandle {
    active: Arc<AtomicBool>,
}

impl Drop for HighlightHandle {
    fn drop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
    }
}

// ─── 高亮框实现（复用原始逻辑） ──────────────────────────────────────────────

mod windows_impl {
    use super::*;
    use std::thread;
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM},
            Graphics::Gdi::{
                BeginPaint, DeleteObject, EndPaint, GetStockObject,
                PAINTSTRUCT, SelectObject, Rectangle, TextOutW,
                CreateSolidBrush, GetTextExtentPoint32W, DEFAULT_GUI_FONT,
            },
            UI::WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow,
                PostQuitMessage, PostMessageW, RegisterClassExW, SetWindowPos,
                ShowWindow, PeekMessageW, TranslateMessage, DispatchMessageW,
                HWND_TOPMOST, WM_DESTROY, WM_PAINT, WM_CLOSE, WNDCLASSEXW,
                WS_EX_LAYERED, WS_EX_TRANSPARENT, WS_EX_TOPMOST, WS_EX_NOACTIVATE,
                WS_POPUP, SWP_NOMOVE, SWP_NOSIZE, SW_SHOW,
                SetLayeredWindowAttributes, LWA_COLORKEY, GetClientRect,
                PM_REMOVE, WM_QUIT,
            },
            System::LibraryLoader::GetModuleHandleW,
        },
    };

    const BORDER_WIDTH: i32 = 3;
    const LABEL_PADDING: i32 = 4;

    const HIGHLIGHT_COLOR: u32 = 0x2E7D32;
    const TEXT_COLOR: u32 = 0xFFFFFF;
    const TRANSPARENT_KEY: u32 = 0x00FFFF;

    const BORDER_CLASS: &str = "ElemSelectorBorder\0";
    const LABEL_CLASS: &str = "ElemSelectorLabel\0";

    static BORDER_HWND: AtomicI32 = AtomicI32::new(0);
    static LABEL_HWND: AtomicI32 = AtomicI32::new(0);

    thread_local! {
        static LABEL_TEXT: std::cell::RefCell<String> = std::cell::RefCell::new(String::new());
    }

    pub fn update_highlight(info: &HighlightInfo) {
        hide_internal();

        let r = info.rect.clone();
        let label = info.control_type_cn.clone();

        thread::spawn(move || {
            LABEL_TEXT.with(|cell| *cell.borrow_mut() = label.clone());

            let border_hwnd = create_border_window(&r);
            let label_hwnd = create_label_window(&r, &label);

            if let (Some(bh), Some(lh)) = (border_hwnd, label_hwnd) {
                BORDER_HWND.store(bh.0 as i32, Ordering::SeqCst);
                LABEL_HWND.store(lh.0 as i32, Ordering::SeqCst);

                unsafe {
                    let _ = ShowWindow(bh, SW_SHOW);
                    let _ = ShowWindow(lh, SW_SHOW);
                }

                while BORDER_HWND.load(Ordering::SeqCst) != 0 {
                    unsafe {
                        let mut msg = std::mem::zeroed();
                        let has_msg = PeekMessageW(&mut msg, Some(bh), 0, 0, PM_REMOVE);
                        if has_msg.as_bool() {
                            if msg.message == WM_QUIT { break; }
                            let _ = TranslateMessage(&msg);
                            let _ = DispatchMessageW(&msg);
                        }
                        let has_msg2 = PeekMessageW(&mut msg, Some(lh), 0, 0, PM_REMOVE);
                        if has_msg2.as_bool() {
                            if msg.message == WM_QUIT { break; }
                            let _ = TranslateMessage(&msg);
                            let _ = DispatchMessageW(&msg);
                        }
                    }
                    thread::sleep(std::time::Duration::from_millis(30));
                }

                BORDER_HWND.store(0, Ordering::SeqCst);
                LABEL_HWND.store(0, Ordering::SeqCst);
                unsafe {
                    let _ = DestroyWindow(bh);
                    let _ = DestroyWindow(lh);
                }
            }
        });
    }

    pub fn flash_with_info(info: &HighlightInfo, duration_ms: u64) {
        hide_internal();

        let r = info.rect.clone();
        let label = info.control_type_cn.clone();

        thread::spawn(move || {
            LABEL_TEXT.with(|cell| *cell.borrow_mut() = label.clone());

            let border_hwnd = create_border_window(&r);
            let label_hwnd = create_label_window(&r, &label);

            if let (Some(bh), Some(lh)) = (border_hwnd, label_hwnd) {
                BORDER_HWND.store(bh.0 as i32, Ordering::SeqCst);
                LABEL_HWND.store(lh.0 as i32, Ordering::SeqCst);

                unsafe {
                    let _ = ShowWindow(bh, SW_SHOW);
                    let _ = ShowWindow(lh, SW_SHOW);
                }

                let start = std::time::Instant::now();
                let duration = std::time::Duration::from_millis(duration_ms);
                while start.elapsed() < duration && BORDER_HWND.load(Ordering::SeqCst) != 0 {
                    unsafe {
                        let mut msg = std::mem::zeroed();
                        let has_msg = PeekMessageW(&mut msg, Some(bh), 0, 0, PM_REMOVE);
                        if has_msg.as_bool() {
                            if msg.message == WM_QUIT { break; }
                            let _ = TranslateMessage(&msg);
                            let _ = DispatchMessageW(&msg);
                        }
                        let has_msg2 = PeekMessageW(&mut msg, Some(lh), 0, 0, PM_REMOVE);
                        if has_msg2.as_bool() {
                            if msg.message == WM_QUIT { break; }
                            let _ = TranslateMessage(&msg);
                            let _ = DispatchMessageW(&msg);
                        }
                    }
                    thread::sleep(std::time::Duration::from_millis(16));
                }

                BORDER_HWND.store(0, Ordering::SeqCst);
                LABEL_HWND.store(0, Ordering::SeqCst);
                unsafe {
                    let _ = DestroyWindow(bh);
                    let _ = DestroyWindow(lh);
                }
            }
        });
    }

    pub fn hide() {
        hide_internal();
    }

    fn hide_internal() {
        let border_val = BORDER_HWND.load(Ordering::SeqCst);
        let label_val = LABEL_HWND.load(Ordering::SeqCst);

        if border_val != 0 {
            BORDER_HWND.store(0, Ordering::SeqCst);
            unsafe { let _ = PostMessageW(Some(HWND(border_val as _)), WM_CLOSE, WPARAM(0), LPARAM(0)); }
        }
        if label_val != 0 {
            LABEL_HWND.store(0, Ordering::SeqCst);
            unsafe { let _ = PostMessageW(Some(HWND(label_val as _)), WM_CLOSE, WPARAM(0), LPARAM(0)); }
        }
    }

    pub fn show(rect: &ElementRect, control_type_cn: &str) -> HighlightHandle {
        hide_internal();

        let active = Arc::new(AtomicBool::new(true));
        let active2 = active.clone();
        let r = rect.clone();
        let label = control_type_cn.to_string();

        thread::spawn(move || {
            LABEL_TEXT.with(|cell| *cell.borrow_mut() = label.clone());

            let border_hwnd = create_border_window(&r);
            let label_hwnd = create_label_window(&r, &label);

            if let (Some(bh), Some(lh)) = (border_hwnd, label_hwnd) {
                BORDER_HWND.store(bh.0 as i32, Ordering::SeqCst);
                LABEL_HWND.store(lh.0 as i32, Ordering::SeqCst);

                unsafe {
                    let _ = ShowWindow(bh, SW_SHOW);
                    let _ = ShowWindow(lh, SW_SHOW);
                }

                while active2.load(Ordering::SeqCst) && BORDER_HWND.load(Ordering::SeqCst) != 0 {
                    unsafe {
                        let mut msg = std::mem::zeroed();
                        let has_msg = PeekMessageW(&mut msg, Some(bh), 0, 0, PM_REMOVE);
                        if has_msg.as_bool() {
                            if msg.message == WM_QUIT { break; }
                            let _ = TranslateMessage(&msg);
                            let _ = DispatchMessageW(&msg);
                        }
                        let has_msg2 = PeekMessageW(&mut msg, Some(lh), 0, 0, PM_REMOVE);
                        if has_msg2.as_bool() {
                            if msg.message == WM_QUIT { break; }
                            let _ = TranslateMessage(&msg);
                            let _ = DispatchMessageW(&msg);
                        }
                    }
                    thread::sleep(std::time::Duration::from_millis(30));
                }

                BORDER_HWND.store(0, Ordering::SeqCst);
                LABEL_HWND.store(0, Ordering::SeqCst);
                unsafe {
                    let _ = DestroyWindow(bh);
                    let _ = DestroyWindow(lh);
                }
            }
        });

        HighlightHandle { active }
    }

    fn create_border_window(rect: &ElementRect) -> Option<HWND> {
        let class: Vec<u16> = BORDER_CLASS.encode_utf16().collect();

        unsafe {
            let h_instance = GetModuleHandleW(None).unwrap_or_default();
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(border_wnd_proc),
                lpszClassName: PCWSTR(class.as_ptr()),
                hInstance: h_instance.into(),
                ..Default::default()
            };
            let _ = RegisterClassExW(&wc);

            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                PCWSTR(class.as_ptr()),
                PCWSTR::null(),
                WS_POPUP,
                rect.x, rect.y, rect.width, rect.height,
                None, None, None, None,
            );

            match hwnd {
                Ok(h) => {
                    let _ = SetLayeredWindowAttributes(h, COLORREF(TRANSPARENT_KEY), 0, LWA_COLORKEY);
                    let _ = SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
                    Some(h)
                }
                Err(e) => {
                    error!("创建边框窗口失败: {e}");
                    None
                }
            }
        }
    }

    unsafe extern "system" fn border_wnd_proc(
        hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM
    ) -> LRESULT {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);

                let bg_brush = CreateSolidBrush(COLORREF(TRANSPARENT_KEY));
                let border_brush = CreateSolidBrush(COLORREF(HIGHLIGHT_COLOR));

                let old_bg = SelectObject(hdc, bg_brush.into());
                let mut rc = RECT::default();
                if GetClientRect(hwnd, &mut rc).is_ok() {
                    let _ = Rectangle(hdc, rc.left, rc.top, rc.right, rc.bottom);
                }

                let old_border = SelectObject(hdc, border_brush.into());
                let _ = Rectangle(hdc, 0, 0, rc.right, BORDER_WIDTH);
                let _ = Rectangle(hdc, 0, rc.bottom - BORDER_WIDTH, rc.right, rc.bottom);
                let _ = Rectangle(hdc, 0, 0, BORDER_WIDTH, rc.bottom);
                let _ = Rectangle(hdc, rc.right - BORDER_WIDTH, 0, rc.right, rc.bottom);

                SelectObject(hdc, old_bg);
                SelectObject(hdc, old_border);
                let _ = DeleteObject(bg_brush.into());
                let _ = DeleteObject(border_brush.into());

                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_CLOSE => {
                BORDER_HWND.store(0, Ordering::SeqCst);
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                BORDER_HWND.store(0, Ordering::SeqCst);
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    fn create_label_window(rect: &ElementRect, label: &str) -> Option<HWND> {
        let class: Vec<u16> = LABEL_CLASS.encode_utf16().collect();

        let (label_width, label_height) = estimate_label_size(label);

        unsafe {
            let h_instance = GetModuleHandleW(None).unwrap_or_default();
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(label_wnd_proc),
                lpszClassName: PCWSTR(class.as_ptr()),
                hInstance: h_instance.into(),
                ..Default::default()
            };
            let _ = RegisterClassExW(&wc);

            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                PCWSTR(class.as_ptr()),
                PCWSTR::null(),
                WS_POPUP,
                rect.x,
                rect.y - label_height,
                label_width,
                label_height,
                None, None, None, None,
            );

            match hwnd {
                Ok(h) => {
                    use windows::Win32::UI::WindowsAndMessaging::LWA_ALPHA;
                    let _ = SetLayeredWindowAttributes(h, COLORREF(0), 255, LWA_ALPHA);
                    let _ = SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
                    Some(h)
                }
                Err(e) => {
                    error!("创建标签窗口失败: {e}");
                    None
                }
            }
        }
    }

    fn estimate_label_size(label: &str) -> (i32, i32) {
        let mut width = 0;
        for c in label.chars() {
            width += if c.is_ascii() { 8 } else { 14 };
        }
        (width + LABEL_PADDING * 2, 18 + LABEL_PADDING)
    }

    unsafe extern "system" fn label_wnd_proc(
        hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM
    ) -> LRESULT {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);

                let font = GetStockObject(DEFAULT_GUI_FONT);
                let old_font = SelectObject(hdc, font);

                let bg_brush = CreateSolidBrush(COLORREF(HIGHLIGHT_COLOR));
                let old_bg = SelectObject(hdc, bg_brush.into());

                let mut rc = RECT::default();
                if GetClientRect(hwnd, &mut rc).is_ok() {
                    let _ = Rectangle(hdc, rc.left, rc.top, rc.right, rc.bottom);
                }

                use windows::Win32::Graphics::Gdi::{SetBkColor, SetTextColor};
                let _ = SetBkColor(hdc, COLORREF(HIGHLIGHT_COLOR));
                let _ = SetTextColor(hdc, COLORREF(TEXT_COLOR));

                let label = LABEL_TEXT.with(|cell| cell.borrow().clone());
                if !label.is_empty() {
                    let label_wide: Vec<u16> = label.encode_utf16().collect();

                    let mut text_size = windows::Win32::Foundation::SIZE { cx: 0, cy: 0 };
                    let _ = GetTextExtentPoint32W(hdc, &label_wide, &mut text_size);

                    let window_width = rc.right - rc.left;
                    let window_height = rc.bottom - rc.top;
                    let x = (window_width - text_size.cx) / 2;
                    let y = (window_height - text_size.cy) / 2;

                    let _ = TextOutW(hdc, x, y, &label_wide);
                }

                SelectObject(hdc, old_bg);
                SelectObject(hdc, old_font);
                let _ = DeleteObject(bg_brush.into());

                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_CLOSE => {
                LABEL_HWND.store(0, Ordering::SeqCst);
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                LABEL_HWND.store(0, Ordering::SeqCst);
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

// ─── 红色圆点标记实现（点击留痕） ──────────────────────────────────────────────

mod point_impl {
    use super::*;
    use std::thread;
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM},
            Graphics::Gdi::{
                BeginPaint, CreateSolidBrush, DeleteObject, Ellipse,
                EndPaint, SelectObject, PAINTSTRUCT,
            },
            UI::WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow,
                PostQuitMessage, PostMessageW, RegisterClassExW, SetWindowPos,
                ShowWindow, PeekMessageW, TranslateMessage, DispatchMessageW,
                HWND_TOPMOST, WM_DESTROY, WM_PAINT, WM_CLOSE, WNDCLASSEXW,
                WS_EX_LAYERED, WS_EX_TRANSPARENT, WS_EX_TOPMOST, WS_EX_NOACTIVATE,
                WS_POPUP, SWP_NOMOVE, SWP_NOSIZE, SW_SHOW,
                SetLayeredWindowAttributes, LWA_ALPHA, GetClientRect,
                PM_REMOVE, WM_QUIT,
            },
            System::LibraryLoader::GetModuleHandleW,
        },
    };

    const MARK_SIZE: i32 = 16;        // 圆点窗口尺寸
    const MARK_COLOR: u32 = 0x0000FF; // 红色 (BGR)
    const POINT_CLASS: &str = "ElemSelectorPoint\0";

    static POINT_HWND: AtomicI32 = AtomicI32::new(0);

    pub fn flash_point(x: i32, y: i32, duration_ms: u64) {
        // 先关闭旧的圆点窗口
        let old = POINT_HWND.load(Ordering::SeqCst);
        if old != 0 {
            POINT_HWND.store(0, Ordering::SeqCst);
            unsafe {
                let _ = PostMessageW(Some(HWND(old as _)), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            // 短暂等待旧窗口关闭
            thread::sleep(std::time::Duration::from_millis(50));
        }

        let half = MARK_SIZE / 2;

        thread::spawn(move || {
            let hwnd = create_point_window(x - half, y - half);

            if let Some(h) = hwnd {
                POINT_HWND.store(h.0 as i32, Ordering::SeqCst);

                unsafe {
                    let _ = ShowWindow(h, SW_SHOW);
                }

                let start = std::time::Instant::now();
                let duration = std::time::Duration::from_millis(duration_ms);
                while start.elapsed() < duration && POINT_HWND.load(Ordering::SeqCst) != 0 {
                    unsafe {
                        let mut msg = std::mem::zeroed();
                        let has_msg = PeekMessageW(&mut msg, Some(h), 0, 0, PM_REMOVE);
                        if has_msg.as_bool() {
                            if msg.message == WM_QUIT { break; }
                            let _ = TranslateMessage(&msg);
                            let _ = DispatchMessageW(&msg);
                        }
                    }
                    thread::sleep(std::time::Duration::from_millis(16));
                }

                POINT_HWND.store(0, Ordering::SeqCst);
                unsafe {
                    let _ = DestroyWindow(h);
                }
            }
        });
    }

    fn create_point_window(x: i32, y: i32) -> Option<HWND> {
        let class: Vec<u16> = POINT_CLASS.encode_utf16().collect();

        unsafe {
            let h_instance = GetModuleHandleW(None).unwrap_or_default();
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(point_wnd_proc),
                lpszClassName: PCWSTR(class.as_ptr()),
                hInstance: h_instance.into(),
                ..Default::default()
            };
            let _ = RegisterClassExW(&wc);

            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                PCWSTR(class.as_ptr()),
                PCWSTR::null(),
                WS_POPUP,
                x, y, MARK_SIZE, MARK_SIZE,
                None, None, None, None,
            );

            match hwnd {
                Ok(h) => {
                    // 使用 alpha 透明度，完全可见
                    let _ = SetLayeredWindowAttributes(h, COLORREF(0), 255, LWA_ALPHA);
                    let _ = SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
                    Some(h)
                }
                Err(e) => {
                    error!("创建圆点窗口失败: {e}");
                    None
                }
            }
        }
    }

    unsafe extern "system" fn point_wnd_proc(
        hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM
    ) -> LRESULT {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);

                let brush = CreateSolidBrush(COLORREF(MARK_COLOR));
                let old_brush = SelectObject(hdc, brush.into());

                let mut rc = RECT::default();
                if GetClientRect(hwnd, &mut rc).is_ok() {
                    // 绘制实心圆（椭圆）
                    let _ = Ellipse(hdc, rc.left, rc.top, rc.right, rc.bottom);
                }

                SelectObject(hdc, old_brush);
                let _ = DeleteObject(brush.into());

                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_CLOSE => {
                POINT_HWND.store(0, Ordering::SeqCst);
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                POINT_HWND.store(0, Ordering::SeqCst);
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
