// src/gui/mod.rs
//
// GUI 层 - 桌面应用专用模块
// iced 迁移后仅保留底层组件

pub mod capture_overlay;
pub mod highlight;
pub mod multi_highlight;
pub mod logger;
pub mod input_hook;
pub mod raw_input;
pub mod state_model;

// 类型定义（iced 和旧 GUI 共用）
pub mod types;

// iced GUI 应用
pub mod iced_app;
pub mod iced_style;
pub mod persistence;

// 旧 egui 模块（已停用，保留源码参考）
// pub mod app;
// pub mod theme;
// pub mod helpers;
// pub mod layout;

// Re-export iced GUI entry points for consumers that use `gui::*`
#[allow(unused_imports)]
pub use iced_app::{update, view, subscription, State};