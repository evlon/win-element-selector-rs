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

/// 超轻量级检查窗口是否存在（仅使用Win32 API，不涉及UIA）
pub fn quick_check_window_exists(window_selector: &str) -> bool {
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, IsWindowVisible, GetWindowTextW, GetClassNameW, GetWindowThreadProcessId};
    
    // 解析窗口选择器获取关键属性
    let (expected_name, expected_class, expected_process) = parse_window_selector(window_selector);
    
    // 如果没有任何过滤条件，认为有效
    if expected_name.is_none() && expected_class.is_none() && expected_process.is_none() {
        return true;
    }
    
    // 使用 EnumWindows 快速检查是否有匹配的窗口
    struct SearchState {
        expected_name: Option<String>,
        expected_class: Option<String>,
        expected_process: Option<String>,
        found: bool,
    }
    
    let state = std::cell::RefCell::new(SearchState {
        expected_name,
        expected_class,
        expected_process,
        found: false,
    });
    
    unsafe extern "system" fn enum_callback(
        hwnd: HWND, 
        lparam: LPARAM,
    ) -> windows::core::BOOL {
        let state = &*(lparam.0 as *const std::cell::RefCell<SearchState>);
        let mut state = state.borrow_mut();
        
        // 只检查可见窗口
        if !IsWindowVisible(hwnd).as_bool() {
            return windows::core::BOOL(1);
        }
        
        // 检查进程名（如果需要）
        if let Some(ref expected_proc) = state.expected_process {
            let mut pid: u32 = 0;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
            if pid != 0 {
                let proc_name = get_process_name_by_id(pid);
                if &proc_name != expected_proc {
                    return windows::core::BOOL(1);
                }
            } else {
                return windows::core::BOOL(1);
            }
        }
        
        // 检查窗口标题
        if let Some(ref expected) = state.expected_name {
            let mut buffer = [0u16; 512];
            let len = GetWindowTextW(hwnd, &mut buffer);
            if len > 0 {
                let title = String::from_utf16_lossy(&buffer[..len as usize]);
                if &title != expected {
                    return windows::core::BOOL(1);
                }
            } else {
                return windows::core::BOOL(1);
            }
        }
        
        // 检查类名
        if let Some(ref expected) = state.expected_class {
            let mut buffer = [0u16; 256];
            let len = GetClassNameW(hwnd, &mut buffer);
            if len > 0 {
                let class = String::from_utf16_lossy(&buffer[..len as usize]);
                if &class != expected {
                    return windows::core::BOOL(1);
                }
            } else {
                return windows::core::BOOL(1);
            }
        }
        
        // 找到匹配的窗口
        state.found = true;
        windows::core::BOOL(0) // 停止枚举
    }
    
    let state_ptr = &state as *const _ as isize;
    
    unsafe {
        let _ = EnumWindows(Some(enum_callback), LPARAM(state_ptr));
    }
    
    state.into_inner().found
}

/// 解析窗口选择器，提取 Name、ClassName、ProcessName
fn parse_window_selector(selector: &str) -> (Option<String>, Option<String>, Option<String>) {
    let mut name = None;
    let mut class = None;
    let mut process_name = None;
    
    // Extract content between [ and ]
    if let Some(start) = selector.find('[') {
        if let Some(end) = selector.rfind(']') {
            let predicates = &selector[start + 1..end];
            
            // Parse @Name='value'
            if let Some(pos) = predicates.find("@Name='") {
                let start_pos = pos + 7;
                if let Some(end_pos) = predicates[start_pos..].find('\'') {
                    name = Some(predicates[start_pos..start_pos + end_pos].to_string());
                }
            }
            
            // Parse @ClassName='value'
            if let Some(pos) = predicates.find("@ClassName='") {
                let start_pos = pos + 12;
                if let Some(end_pos) = predicates[start_pos..].find('\'') {
                    class = Some(predicates[start_pos..start_pos + end_pos].to_string());
                }
            }
            
            // Parse @ProcessName='value'
            if let Some(pos) = predicates.find("@ProcessName='") {
                let start_pos = pos + 14;
                if let Some(end_pos) = predicates[start_pos..].find('\'') {
                    process_name = Some(predicates[start_pos..start_pos + end_pos].to_string());
                }
            }
        }
    }
    
    (name, class, process_name)
}

/// 根据进程ID获取进程名称
fn get_process_name_by_id(process_id: u32) -> String {
    use windows::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    unsafe {
        let handle_result = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id);
        let handle = match handle_result {
            Ok(h) => h,
            Err(_) => return String::new(),
        };
        
        if handle == HANDLE::default() {
            return String::new();
        }

        let mut buffer = [0u16; 260]; // MAX_PATH
        let mut length = buffer.len() as u32;
        
        let result = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buffer.as_mut_ptr()),
            &mut length,
        );
        
        let _ = CloseHandle(handle);
        
        if result.is_ok() && length > 0 {
            // Get just the filename without path
            let full_path = OsString::from_wide(&buffer[..length as usize]);
            if let Some(path) = full_path.to_str() {
                if let Some(filename) = path.rsplit('\\').next() {
                    // Remove .exe extension
                    return filename.strip_suffix(".exe")
                        .unwrap_or(filename)
                        .to_string();
                }
            }
        }
    }
    
    String::new()
}