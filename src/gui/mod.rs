// src/gui/mod.rs
//
// GUI 层 - 桌面应用专用模块
// 仅由 main.rs 使用

pub mod app;
pub mod capture_overlay;
pub mod highlight;
pub mod mouse_hook;

// Re-export for convenience
pub use app::SelectorApp;