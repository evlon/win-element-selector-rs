// src/core/uia/mod.rs
//
// Windows UI Automation core operations.
// Shared between GUI and HTTP API.
//
// XPath execution uses uiauto-xpath library for full XPath 1.0 standard support.

// Allow non-upper-case globals for UIA constants from windows crate.
#![allow(non_upper_case_globals)]

use super::model::{CaptureMode, CaptureResult, DetailedValidationResult, ElementRect, HierarchyNode, LayerValidationResult, Operator, PredicateFailure, PropertyValidationResult, SegmentValidationResult, ValidationResult, WalkerHint, WindowInfo};
use log::{debug, error, info};
use regex::Regex;
use uiauto_xpath::{XPath, UiElement as UiaXPathElement, control_type_id_to_name, control_type_name_to_id};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use windows::core::Interface;
use windows::{
    core::BSTR,
    Win32::{
        Foundation::{POINT, HWND, LPARAM, RECT},
        System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
        UI::{
            Accessibility::{
                CUIAutomation, IUIAutomation, IUIAutomationElement,
                IUIAutomationTreeWalker,
            },
            WindowsAndMessaging::{
                GetCursorPos, EnumChildWindows, EnumWindows, GetWindowThreadProcessId,
                IsWindowVisible,
            },
        },
    },
};

// ═══════════════════════════════════════════════════════════════════════════════
// Sub-modules
// ═══════════════════════════════════════════════════════════════════════════════
pub mod helpers;
pub mod capture;
pub mod validation;
pub mod window;
pub mod element;
pub mod cache;
pub mod find;
pub mod navigation;
pub mod inspect;
pub mod visibility;

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
        capture_mode: CaptureMode::Fast,
    }
}
