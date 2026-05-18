// src/capture.rs
//
// Capture 公共 API 包装层
// GUI 专用 - 调用 core::uia 模块

use crate::core::uia;
use crate::core::model::{DetailedValidationResult, WindowInfo};

// 公开导出 CaptureResult，供 GUI 使用
pub use crate::core::model::CaptureResult;

/// Mock capture result for testing.
pub fn mock() -> CaptureResult {
    uia::mock()
}

/// Capture the element under the mouse cursor.
#[allow(dead_code)]
pub fn capture() -> CaptureResult {
    uia::capture_at_cursor()
}

/// Capture the element at a specific screen coordinate.
#[allow(dead_code)]
pub fn capture_at(x: i32, y: i32) -> CaptureResult {
    uia::capture_at_point(x, y)
}

/// Validate using window selector and element XPath with detailed per-segment results.
pub fn validate_selector_and_xpath_detailed(
    window_selector: &str,
    element_xpath: &str,
    hierarchy: &[crate::core::model::HierarchyNode],
) -> DetailedValidationResult {
    uia::validate_selector_and_xpath_detailed(window_selector, element_xpath, hierarchy)
}

/// Find all matching elements with detailed info
pub fn find_all_elements_detailed(
    window_selector: &str,
    element_xpath: &str,
    random_range: f32,
) -> Vec<crate::api::types::ElementInfo> {
    uia::find_all_elements_detailed(window_selector, element_xpath, random_range)
}

/// Enumerate all top-level windows on desktop.
/// Uses Win32 EnumWindows API - fast and matches Alt+Tab
pub fn list_windows() -> Vec<WindowInfo> {
    crate::core::enum_windows::enumerate_windows_fast()
}