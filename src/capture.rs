// src/capture.rs
//
// Capture 公共 API 包装层
// GUI 专用 - 所有 UIA 操作通过 ComWorker 执行

use crate::core::model::{DetailedValidationResult, WindowInfo};

// 公开导出 CaptureResult，供 GUI 使用
pub use crate::core::model::CaptureResult;

/// Mock capture result for testing.
pub fn mock() -> CaptureResult {
    // 测试用，保持原有实现
    crate::core::uia::mock()
}

/// Capture the element under the mouse cursor.
#[allow(dead_code)]
pub fn capture() -> CaptureResult {
    // 通过 ComWorker 执行
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
            };
        }
        p
    };
    
    match crate::core::com_worker::global_capture_at(pt.x, pt.y) {
        Ok(result) => result,
        Err(e) => CaptureResult {
            hierarchy: vec![],
            cursor_x: pt.x, cursor_y: pt.y,
            error: Some(format!("捕获失败: {}", e)),
            window_info: None,
        },
    }
}

/// Capture the element at a specific screen coordinate.
#[allow(dead_code)]
pub fn capture_at(x: i32, y: i32) -> CaptureResult {
    // 通过 ComWorker 执行
    match crate::core::com_worker::global_capture_at(x, y) {
        Ok(result) => result,
        Err(e) => CaptureResult {
            hierarchy: vec![],
            cursor_x: x, cursor_y: y,
            error: Some(format!("捕获失败: {}", e)),
            window_info: None,
        },
    }
}

/// Validate using window selector and element XPath with detailed per-segment results.
pub fn validate_selector_and_xpath_detailed(
    window_selector: &str,
    element_xpath: &str,
    hierarchy: &[crate::core::model::HierarchyNode],
) -> DetailedValidationResult {
    // 通过 ComWorker 执行
    match crate::core::com_worker::global_validate_xpath(
        window_selector.to_string(),
        element_xpath.to_string(),
        hierarchy.to_vec(),
    ) {
        Ok(result) => result,
        Err(e) => DetailedValidationResult {
            overall: crate::core::model::ValidationResult::Error(e.to_string()),
            segments: vec![],
            layers: vec![],
            total_duration_ms: 0,
        },
    }
}

/// Find all matching elements with detailed info
pub fn find_all_elements_detailed(
    window_selector: &str,
    element_xpath: &str,
    random_range: f32,
) -> Vec<crate::api::types::ElementInfo> {
    // 通过 ComWorker 执行
    match crate::core::com_worker::global_find_element(
        window_selector.to_string(),
        element_xpath.to_string(),
        Some(random_range),
    ) {
        Ok(results) => results,
        Err(e) => {
            log::error!("find_all_elements_detailed failed: {}", e);
            vec![]
        },
    }
}

/// Enumerate all top-level windows on desktop.
/// Uses Win32 EnumWindows API - fast and matches Alt+Tab
pub fn list_windows() -> Vec<WindowInfo> {
    crate::core::enum_windows::enumerate_windows_fast()
}

/// 查找共同元素（基于共同祖先链 XPath）
pub fn find_common_elements(window_selector: &str, xpath: &str) -> Vec<crate::api::types::ElementInfo> {
    match crate::core::com_worker::global_find_common_elements(
        window_selector.to_string(),
        xpath.to_string(),
    ) {
        Ok(results) => results,
        Err(e) => {
            log::error!("find_common_elements failed: {}", e);
            vec![]
        },
    }
}