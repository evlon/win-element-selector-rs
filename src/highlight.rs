// src/highlight.rs
//
// 元素高亮显示 - 使用两个独立窗口：
// 1. 标签窗口：显示元素类型，宽度自适应文字
// 2. 高亮框窗口：纯边框，中空透明

use crate::model::{ElementRect, HighlightInfo};
use log::error;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Arc;

// ─── Public API ──────────────────────────────────────────────────────────────

pub fn flash(rect: &ElementRect, duration_ms: u64) {
    let info = HighlightInfo::new(rect.clone(), "");
    flash_with_info(&info, duration_ms);
}

pub fn flash_with_info(info: &HighlightInfo, duration_ms: u64) {
    #[cfg(target_os = "windows")]
    windows_impl::flash_with_info(info, duration_ms);

    #[cfg(not(target_os = "windows"))]
    log::debug!("highlight::flash_with_info (stub)");
}

pub fn hide() {
    #[cfg(target_os = "windows")]
    windows_impl::hide();

    #[cfg(not(target_os = "windows"))]
    log::debug!("highlight::hide (stub)");
}

pub fn update_highlight(info: &HighlightInfo) {
    #[cfg(target_os = "windows")]
    windows_impl::update_highlight(info);

    #[cfg(not(target_os = "windows"))]
    log::debug!("highlight::update_highlight (stub)");
}

#[allow(dead_code)]
pub fn show(rect: &ElementRect) -> HighlightHandle {
    #[cfg(target_os = "windows")]
    {
        windows_impl::show(rect, "")
    }
    #[cfg(not(target_os = "windows"))]
    {
        HighlightHandle { active: Arc::new(AtomicBool::new(true)) }
    }
}

#[allow(dead_code)]
pub fn show_with_info(info: &HighlightInfo) -> HighlightHandle {
    #[cfg(target_os = "windows")]
    {
        windows_impl::show(&info.rect, &info.control_type_cn)
    }
    #[cfg(not(target_os = "windows"))]
    {
        HighlightHandle { active: Arc::new(AtomicBool::new(true)) }
    }
}

pub struct HighlightHandle {
    active: Arc<AtomicBool>,
}

impl Drop for HighlightHandle {
    fn drop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
    }
}

// ─── Windows implementation ──────────────────────────────────────────────────
#[cfg(target_os = "windows")]
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

    // ─── 常量 ────────────────────────────────────────────────────────────────
    const BORDER_WIDTH: i32 = 3;       // 边框宽度
    const LABEL_PADDING: i32 = 4;      // 标签内边距
    
    // 颜色 (BGR格式)
    const HIGHLIGHT_COLOR: u32 = 0x2E7D32;  // 绿色 - 边框和标签背景
    const TEXT_COLOR: u32 = 0xFFFFFF;        // 白色文字
    const TRANSPARENT_KEY: u32 = 0x00FFFF;   // Cyan - 透明色键
    
    // 窗口类名
    const BORDER_CLASS: &str = "ElemSelectorBorder\0";
    const LABEL_CLASS: &str = "ElemSelectorLabel\0";
    
    // 存储两个窗口的 HWND
    static BORDER_HWND: AtomicI32 = AtomicI32::new(0);
    static LABEL_HWND: AtomicI32 = AtomicI32::new(0);
    
    // 线程本地存储标签文字
    thread_local! {
        static LABEL_TEXT: std::cell::RefCell<String> = std::cell::RefCell::new(String::new());
    }

    // ─── 公共函数 ─────────────────────────────────────────────────────────────
    
    pub fn update_highlight(info: &HighlightInfo) {
        hide_internal();
        
        let r = info.rect.clone();
        let label = info.control_type_cn.clone();
        
        thread::spawn(move || {
            LABEL_TEXT.with(|cell| *cell.borrow_mut() = label.clone());
            
            // 创建两个窗口
            let border_hwnd = create_border_window(&r);
            let label_hwnd = create_label_window(&r, &label);
            
            if let (Some(bh), Some(lh)) = (border_hwnd, label_hwnd) {
                BORDER_HWND.store(bh.0 as i32, Ordering::SeqCst);
                LABEL_HWND.store(lh.0 as i32, Ordering::SeqCst);
                
                unsafe {
                    let _ = ShowWindow(bh, SW_SHOW);
                    let _ = ShowWindow(lh, SW_SHOW);
                }
                
                // 消息循环 - 只处理高亮窗口的消息
                while BORDER_HWND.load(Ordering::SeqCst) != 0 {
                    unsafe {
                        // 只从边框窗口获取消息，避免截获其他窗口的消息
                        let mut msg = std::mem::zeroed();
                        let has_msg = PeekMessageW(&mut msg, bh, 0, 0, PM_REMOVE);
                        if has_msg.as_bool() {
                            if msg.message == WM_QUIT { break; }
                            let _ = TranslateMessage(&msg);
                            let _ = DispatchMessageW(&msg);
                        }
                        // 同时检查标签窗口的消息
                        let has_msg2 = PeekMessageW(&mut msg, lh, 0, 0, PM_REMOVE);
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
                        // 只从高亮窗口获取消息
                        let mut msg = std::mem::zeroed();
                        let has_msg = PeekMessageW(&mut msg, bh, 0, 0, PM_REMOVE);
                        if has_msg.as_bool() {
                            if msg.message == WM_QUIT { break; }
                            let _ = TranslateMessage(&msg);
                            let _ = DispatchMessageW(&msg);
                        }
                        let has_msg2 = PeekMessageW(&mut msg, lh, 0, 0, PM_REMOVE);
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
            unsafe { let _ = PostMessageW(HWND(border_val as _), WM_CLOSE, WPARAM(0), LPARAM(0)); }
        }
        if label_val != 0 {
            LABEL_HWND.store(0, Ordering::SeqCst);
            unsafe { let _ = PostMessageW(HWND(label_val as _), WM_CLOSE, WPARAM(0), LPARAM(0)); }
        }
        thread::sleep(std::time::Duration::from_millis(10));
    }

    #[allow(dead_code)]
    pub fn show(rect: &ElementRect, control_type_cn: &str) -> HighlightHandle {
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
                        // 只从高亮窗口获取消息
                        let mut msg = std::mem::zeroed();
                        let has_msg = PeekMessageW(&mut msg, bh, 0, 0, PM_REMOVE);
                        if has_msg.as_bool() {
                            if msg.message == WM_QUIT { break; }
                            let _ = TranslateMessage(&msg);
                            let _ = DispatchMessageW(&msg);
                        }
                        let has_msg2 = PeekMessageW(&mut msg, lh, 0, 0, PM_REMOVE);
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

    // ─── 边框窗口 ──────────────────────────────────────────────────────────────
    
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
                    // 使用颜色键: Cyan 区域透明
                    let _ = SetLayeredWindowAttributes(h, COLORREF(TRANSPARENT_KEY), 0, LWA_COLORKEY);
                    let _ = SetWindowPos(h, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
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
                
                // 绘制绿色边框 (中空)
                // 整个窗口填充 Cyan (透明)，只绘制边框
                let bg_brush = CreateSolidBrush(COLORREF(TRANSPARENT_KEY));
                let border_brush = CreateSolidBrush(COLORREF(HIGHLIGHT_COLOR));
                
                // 填充中心为透明色 (Cyan)
                let old_bg = SelectObject(hdc, bg_brush);
                let mut rc = RECT::default();
                if GetClientRect(hwnd, &mut rc).is_ok() {
                    let _ = Rectangle(hdc, rc.left, rc.top, rc.right, rc.bottom);
                }
                
                // 绘制边框四条边 (绿色)
                let old_border = SelectObject(hdc, border_brush);
                // 上边
                let _ = Rectangle(hdc, 0, 0, rc.right, BORDER_WIDTH);
                // 下边
                let _ = Rectangle(hdc, 0, rc.bottom - BORDER_WIDTH, rc.right, rc.bottom);
                // 左边
                let _ = Rectangle(hdc, 0, 0, BORDER_WIDTH, rc.bottom);
                // 右边
                let _ = Rectangle(hdc, rc.right - BORDER_WIDTH, 0, rc.right, rc.bottom);
                
                SelectObject(hdc, old_bg);
                SelectObject(hdc, old_border);
                let _ = DeleteObject(bg_brush);
                let _ = DeleteObject(border_brush);
                
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

    // ─── 标签窗口 ──────────────────────────────────────────────────────────────
    
    fn create_label_window(rect: &ElementRect, label: &str) -> Option<HWND> {
        let class: Vec<u16> = LABEL_CLASS.encode_utf16().collect();
        
        // 计算标签尺寸
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
            
            // 标签位置：在高亮框上方，紧挨着无间隙
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                PCWSTR(class.as_ptr()),
                PCWSTR::null(),
                WS_POPUP,
                rect.x,  // 左侧与高亮框对齐
                rect.y - label_height,  // 紧挨高亮框顶部，无间隙
                label_width,
                label_height,
                None, None, None, None,
            );
            
            match hwnd {
                Ok(h) => {
                    // 标签窗口使用 alpha 透明度，完全可见
                    // 不使用 LWA_COLORKEY，整个窗口都是实色的
                    use windows::Win32::UI::WindowsAndMessaging::LWA_ALPHA;
                    let _ = SetLayeredWindowAttributes(h, COLORREF(0), 255, LWA_ALPHA);
                    let _ = SetWindowPos(h, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
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
        // 估算：中文 ~14px，英文 ~8px
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
                
                // 使用小字体
                let font = GetStockObject(DEFAULT_GUI_FONT);
                let old_font = SelectObject(hdc, font);
                
                // 绘制绿色背景
                let bg_brush = CreateSolidBrush(COLORREF(HIGHLIGHT_COLOR));
                let old_bg = SelectObject(hdc, bg_brush);
                
                let mut rc = RECT::default();
                if GetClientRect(hwnd, &mut rc).is_ok() {
                    // 填充绿色背景 (无边框)
                    let _ = Rectangle(hdc, rc.left, rc.top, rc.right, rc.bottom);
                }
                
                // 绘制白色文字
                use windows::Win32::Graphics::Gdi::{SetBkColor, SetTextColor};
                let _ = SetBkColor(hdc, COLORREF(HIGHLIGHT_COLOR));  // 绿色背景
                let _ = SetTextColor(hdc, COLORREF(TEXT_COLOR));     // 白色文字
                
                let label = LABEL_TEXT.with(|cell| cell.borrow().clone());
                if !label.is_empty() {
                    let label_wide: Vec<u16> = label.encode_utf16().collect();
                    
                    // 计算文字尺寸
                    let mut text_size = windows::Win32::Foundation::SIZE { cx: 0, cy: 0 };
                    let _ = GetTextExtentPoint32W(hdc, &label_wide, &mut text_size);
                    
                    // 计算居中位置（上下左右都居中）
                    let window_width = rc.right - rc.left;
                    let window_height = rc.bottom - rc.top;
                    let x = (window_width - text_size.cx) / 2;
                    let y = (window_height - text_size.cy) / 2;
                    
                    let _ = TextOutW(hdc, x, y, &label_wide);
                }
                
                SelectObject(hdc, old_bg);
                SelectObject(hdc, old_font);
                let _ = DeleteObject(bg_brush);
                
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