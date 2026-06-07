// src/core/uia/mod.rs
//
// Windows UI Automation core operations.
// Shared between GUI and HTTP API.
//
// XPath execution uses uiauto-xpath library for full XPath 1.0 standard support.
//
// IMPORTANT: All UIA operations use uiautomation-rs safe wrappers.
// windows-rs is only used for non-UIA Win32 APIs (EnumWindows, GetCursorPos, etc.)

use super::model::{CaptureMode, CaptureResult, ChildHwndHint, DetailedValidationResult, ElementRect, FindAllFilter, HierarchyNode, LayerValidationResult, LocateMode, NotFoundReason, Operator, PropertyValidationResult, SearchContext, SearchMode, SegmentValidationResult, ValidationResult, WalkerHint, WindowInfo, XPathProperty};
use log::{debug, error, info};
use regex::Regex;
use uiauto_xpath::{XPath, UiElement as UiaXPathElement, control_type_name_to_id};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

// Re-export uiautomation-rs types for use in sub-modules
pub use uiautomation::core::{UIAutomation, UIElement, UITreeWalker, UICondition, UICacheRequest};
pub use uiautomation::types::{Rect as UiaRect, Point as UiaPoint, ControlType as UiaControlType, Handle as UiaHandle};
pub use uiautomation::types::TreeScope;
pub use uiautomation::types::UIProperty;
pub use uiautomation::types::PropertyConditionFlags;
pub use uiautomation::variants::Variant;

// Re-export windows-rs types still needed for non-UIA Win32 APIs
pub use windows::Win32::Foundation::{HWND, POINT, RECT, LPARAM};
pub use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, GetClientRect, GetCursorPos, GetWindowThreadProcessId,
    IsWindowVisible,
};
pub use windows::core::BOOL as WinBool;

// ═══════════════════════════════════════════════════════════════════════════════
// Sub-modules
// ═══════════════════════════════════════════════════════════════════════════════
pub mod helpers;
pub mod capture;
pub mod validation;
pub mod window;
pub mod element;
pub mod cache;
pub mod find_control;
pub mod find_raw;
pub mod find;
pub mod navigation;
pub mod inspect;
pub mod visibility;
pub mod actions;

// ─── Public API ──────────────────────────────────────────────────────────────

pub use helpers::*;
pub use capture::*;
pub use validation::*;
pub use window::*;
pub use element::*;
pub use cache::*;
pub use find::*;
pub use navigation::*;
pub use inspect::*;
pub use visibility::*;
pub use actions::*;

// ─── Rich mock data ──────────────────────────────────────────────────────────

pub fn mock() -> CaptureResult {
    CaptureResult {
        hierarchy: vec![
            HierarchyNode::new(
                "Window", "MainAppWindow", "WpfWindow", "My Application  —  文档1",
                0, ElementRect { x: 0, y: 0, width: 1280, height: 800 }, 12345,
            ),
            HierarchyNode::new(
                "Pane", "", "DockPanel", "", 0,
                ElementRect { x: 0, y: 30, width: 1280, height: 770 }, 12345,
            ),
            HierarchyNode::new(
                "ToolBar", "mainToolbar", "ToolBarTray", "主工具栏", 0,
                ElementRect { x: 0, y: 30, width: 1280, height: 36 }, 12345,
            ),
            HierarchyNode::new(
                "Button", "btnSave", "Button", "保存", 2,
                ElementRect { x: 120, y: 34, width: 80, height: 28 }, 12345,
            ),
        ],
        cursor_x: 160,
        cursor_y: 48,
        error: None,
        window_info: Some(WindowInfo {
            title: "My Application  —  文档1".to_string(),
            class_name: "WpfWindow".to_string(),
            process_id: 12345,
            process_name: "MyApp".to_string(),
        }),
        capture_mode: CaptureMode::Normal,
        locate_mode: LocateMode::Fast,
        search_context: SearchContext::default_fast(),
    }
}
