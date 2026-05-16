// src/gui/mod.rs
//
// GUI 层 - 桌面应用专用模块
// 仅由 main.rs 使用

pub mod app;
pub mod capture_overlay;
pub mod highlight;
pub mod logger;
pub mod mouse_hook;
pub mod state_model;

// 新增模块 - 重构后的独立组件
pub mod theme;
pub mod types;
pub mod helpers;
pub mod layout;

// Re-export for convenience
pub use app::SelectorApp;