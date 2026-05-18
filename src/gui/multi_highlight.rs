// src/gui/multi_highlight.rs
//
// 多元素高亮管理器 - 支持同时显示多个高亮框
// 
// 使用场景：
// 1. 批量捕获相似元素时，同时高亮所有匹配的元素
// 2. 校验 XPath 时，高亮所有匹配结果
// 3. 对比多个元素的属性时，同时高亮它们

use element_selector::core::model::ElementRect;
use log::error;
use std::collections::HashMap;
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
            RegisterClassExW, UnregisterClassW, SetWindowPos,
            ShowWindow,
            HWND_TOPMOST, WM_DESTROY, WM_PAINT, WM_CLOSE, WNDCLASSEXW,
            WS_EX_LAYERED, WS_EX_TRANSPARENT, WS_EX_TOPMOST, WS_EX_NOACTIVATE,
            WS_POPUP, SWP_NOMOVE, SWP_NOSIZE, SW_SHOW,
            SetLayeredWindowAttributes, LWA_COLORKEY, GetClientRect,
            SetWindowLongPtrW,
            GetWindowLongPtrW, GWLP_USERDATA,
            CS_HREDRAW, CS_VREDRAW,
        },
        System::LibraryLoader::GetModuleHandleW,
    },
};

// ─── 常量 ────────────────────────────────────────────────────────────────
const BORDER_WIDTH: i32 = 3;
const LABEL_PADDING: i32 = 4;
const HIGHLIGHT_COLOR: u32 = 0x2E7D32;  // 绿色
const TEXT_COLOR: u32 = 0xFFFFFF;        // 白色文字
const TRANSPARENT_KEY: u32 = 0x00FFFF;   // Cyan - 透明色键

/// 单个高亮窗口的句柄和标签
#[derive(Debug)]
struct HighlightWindows {
    border_hwnd: HWND,
    label_hwnd: HWND,
    label_text: String,  // 存储标签文本
}

/// 多元素高亮管理器
pub struct MultiHighlightManager {
    /// 活跃的高亮窗口映射 (id -> windows)
    highlights: HashMap<String, HighlightWindows>,
    /// 是否已注册窗口类
    border_class_registered: bool,
    label_class_registered: bool,
}

impl MultiHighlightManager {
    pub fn new() -> Self {
        let mut manager = Self {
            highlights: HashMap::new(),
            border_class_registered: false,
            label_class_registered: false,
        };
        
        // 预先注册窗口类（只注册一次）
        manager.register_window_classes();
        manager
    }
    
    /// 注册窗口类（只需调用一次）
    fn register_window_classes(&mut self) {
        if self.border_class_registered && self.label_class_registered {
            return; // 已经注册过了
        }
        
        unsafe {
            let h_instance = GetModuleHandleW(None).unwrap_or_default();
            
            // 注册边框窗口类
            if !self.border_class_registered {
                let class_name_wide: Vec<u16> = "MultiBorder".encode_utf16().chain(Some(0)).collect();
                let wc_border = WNDCLASSEXW {
                    cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                    style: CS_HREDRAW | CS_VREDRAW,
                    lpfnWndProc: Some(border_wnd_proc),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: h_instance.into(),
                    hIcon: Default::default(),
                    hCursor: Default::default(),
                    hbrBackground: Default::default(),
                    lpszMenuName: PCWSTR::null(),
                    lpszClassName: PCWSTR(class_name_wide.as_ptr()),
                    hIconSm: Default::default(),
                };
                
                if RegisterClassExW(&wc_border) != 0 {
                    self.border_class_registered = true;
                }
            }
            
            // 注册标签窗口类
            if !self.label_class_registered {
                let class_name_wide: Vec<u16> = "MultiLabel".encode_utf16().chain(Some(0)).collect();
                let wc_label = WNDCLASSEXW {
                    cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                    style: CS_HREDRAW | CS_VREDRAW,
                    lpfnWndProc: Some(label_wnd_proc),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: h_instance.into(),
                    hIcon: Default::default(),
                    hCursor: Default::default(),
                    hbrBackground: Default::default(),
                    lpszMenuName: PCWSTR::null(),
                    lpszClassName: PCWSTR(class_name_wide.as_ptr()),
                    hIconSm: Default::default(),
                };
                
                if RegisterClassExW(&wc_label) != 0 {
                    self.label_class_registered = true;
                }
            }
        }
    }

    /// 添加一个高亮框
    pub fn add(&mut self, id: &str, rect: &ElementRect, label: &str) {
        // 如果已存在，先移除
        if self.highlights.contains_key(id) {
            self.remove(id);
        }

        let border_hwnd = self.create_border_window(rect);
        let label_hwnd = self.create_label_window(rect, label);

        if let (Some(bh), Some(lh)) = (border_hwnd, label_hwnd) {
            unsafe {
                let _ = ShowWindow(bh, SW_SHOW);
                let _ = ShowWindow(lh, SW_SHOW);
            }

            self.highlights.insert(
                id.to_string(),
                HighlightWindows {
                    border_hwnd: bh,
                    label_hwnd: lh,
                    label_text: label.to_string(),  // 保存标签文本
                },
            );
        }
    }

    /// 移除一个高亮框
    pub fn remove(&mut self, id: &str) {
        if let Some(windows) = self.highlights.remove(id) {
            unsafe {
                let _ = DestroyWindow(windows.border_hwnd);
                let _ = DestroyWindow(windows.label_hwnd);
            }
        }
    }

    /// 清除所有高亮框
    pub fn clear(&mut self) {
        let ids: Vec<String> = self.highlights.keys().cloned().collect();
        for id in ids {
            self.remove(&id);
        }
    }

    /// 更新某个高亮框的位置
    pub fn update(&mut self, id: &str, rect: &ElementRect, label: &str) {
        self.remove(id);
        self.add(id, rect, label);
    }

    /// 获取活跃的高亮框数量
    pub fn count(&self) -> usize {
        self.highlights.len()
    }

    /// 批量添加多个高亮框
    pub fn add_multiple(&mut self, items: &[(&str, &ElementRect, &str)]) {
        for (id, rect, label) in items {
            self.add(id, rect, label);
        }
    }

    // ─── 内部方法 ──────────────────────────────────────────────────────────

    fn create_border_window(&self, rect: &ElementRect) -> Option<HWND> {
        let class_name_wide: Vec<u16> = "MultiBorder".encode_utf16().chain(Some(0)).collect();

        unsafe {
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                PCWSTR(class_name_wide.as_ptr()),
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

    fn create_label_window(&self, rect: &ElementRect, label: &str) -> Option<HWND> {
        let class_name_wide: Vec<u16> = "MultiLabel".encode_utf16().chain(Some(0)).collect();

        let (label_width, label_height) = estimate_label_size(label);

        unsafe {
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                PCWSTR(class_name_wide.as_ptr()),
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
                    // 【关键】将标签文本存储到窗口用户数据中
                    let label_box = Box::new(label.to_string());
                    let ptr = Box::into_raw(label_box) as isize;
                    SetWindowLongPtrW(h, GWLP_USERDATA, ptr);
                    
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
}

impl Drop for MultiHighlightManager {
    fn drop(&mut self) {
        self.clear();
        
        // 注销窗口类，释放系统资源
        unsafe {
            let h_instance = GetModuleHandleW(None).unwrap_or_default();
            let h_inst: windows::Win32::Foundation::HINSTANCE = h_instance.into();
            
            if self.border_class_registered {
                let class_name_wide: Vec<u16> = "MultiBorder".encode_utf16().chain(Some(0)).collect();
                let _ = UnregisterClassW(PCWSTR(class_name_wide.as_ptr()), Some(h_inst));
            }
            if self.label_class_registered {
                let class_name_wide: Vec<u16> = "MultiLabel".encode_utf16().chain(Some(0)).collect();
                let _ = UnregisterClassW(PCWSTR(class_name_wide.as_ptr()), Some(h_inst));
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
        WM_CLOSE | WM_DESTROY => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
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

            // 【关键】从窗口用户数据中获取标签文本
            let label_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const String;
            if !label_ptr.is_null() {
                let label = unsafe { &*label_ptr };
                let label_wide: Vec<u16> = label.encode_utf16().collect();
                
                // 计算文字尺寸
                let mut text_size = windows::Win32::Foundation::SIZE { cx: 0, cy: 0 };
                let _ = GetTextExtentPoint32W(hdc, &label_wide, &mut text_size);
                
                // 计算居中位置
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
        WM_CLOSE | WM_DESTROY => {
            // 【关键】清理窗口用户数据中的字符串
            let label_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut String;
            if !label_ptr.is_null() {
                unsafe {
                    let _ = Box::from_raw(label_ptr);  // 释放内存
                }
            }
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
