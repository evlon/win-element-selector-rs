// src/gui/mod.rs
//
// GUI 层 - 桌面应用专用模块
// iced 实现

pub mod capture_overlay;
pub mod highlight;
pub mod multi_highlight;
pub mod logger;
pub mod input_hook;
pub mod raw_input;

// 类型定义
pub mod types;

// iced GUI 应用
pub mod iced_app;
pub mod iced_style;
pub mod persistence;

// Re-export iced GUI entry points
#[allow(unused_imports)]
pub use iced_app::{update, view, subscription, State};