// src/capture.rs
//
// Capture 公共 API 包装层
// GUI 专用 - 调用 core::uia 模块

use crate::core::uia;
use crate::core::model::{CaptureResult, DetailedValidationResult, WindowInfo};

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
) -> DetailedValidationResult {
    uia::validate_selector_and_xpath_detailed(window_selector, element_xpath)
}

/// Enumerate all top-level windows on desktop.
pub fn list_windows() -> Vec<WindowInfo> {
    uia::enumerate_windows()
}