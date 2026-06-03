// src/capture.rs
//
// Capture 公共 API 包装层
// GUI 和 API 共用 — 直接调用 core::uia 层

use crate::core::model::{CaptureMode, DetailedValidationResult, WindowInfo};

// 公开导出 CaptureResult，供 GUI 使用
pub use crate::core::model::CaptureResult;

/// Mock capture result for testing.
pub fn mock() -> CaptureResult {
    crate::core::uia::mock()
}

/// Capture the element under the mouse cursor (standard mode).
pub fn capture() -> CaptureResult {
    let pt = unsafe {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut p = POINT::default();
        if GetCursorPos(&mut p).is_err() {
            return CaptureResult {
                hierarchy: vec![],
                cursor_x: 0, cursor_y: 0,
                error: Some("GetCursorPos 失败".to_string()),
                window_info: None,
                capture_mode: CaptureMode::Fast,
            };
        }
        p
    };
    crate::core::uia::capture_at_point(pt.x, pt.y)
}

/// Capture the element at a specific screen coordinate (standard mode).
pub fn capture_at(x: i32, y: i32) -> CaptureResult {
    crate::core::uia::capture_at_point(x, y)
}

/// Enhanced capture: uses RawViewWalker + RECT hit-test to find the innermost element.
/// Useful for WebView-based apps where ElementFromPoint returns a wrapper element.
pub fn capture_enhanced_at(x: i32, y: i32) -> CaptureResult {
    crate::core::uia::capture_enhanced_at_point(x, y)
}

/// Validate using window selector and element XPath with detailed per-segment results.
pub fn validate_selector_and_xpath_detailed(
    window_selector: &str,
    element_xpath: &str,
    hierarchy: &[crate::core::model::HierarchyNode],
) -> DetailedValidationResult {
    crate::core::uia::validate_selector_and_xpath_detailed(window_selector, element_xpath, hierarchy)
}

/// Find all matching elements with detailed info
pub fn find_all_elements_detailed(
    window_selector: &str,
    element_xpath: &str,
    random_range: f32,
) -> Vec<crate::api::types::ElementInfo> {
    crate::core::uia::find_all_elements_detailed(window_selector, element_xpath, random_range)
}

/// Enumerate all top-level windows on desktop.
/// Uses Win32 EnumWindows API - fast and matches Alt+Tab
pub fn list_windows() -> Vec<WindowInfo> {
    crate::core::enum_windows::enumerate_windows_fast()
}

/// 查找共同元素（基于共同祖先链 XPath）
pub fn find_common_elements(_window_selector: &str, xpath: &str) -> Vec<crate::api::types::ElementInfo> {
    crate::core::uia::find_all_elements_from_root(xpath, 5.0)
}
