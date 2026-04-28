// src/lib.rs
//
// 库模块入口 - 公共模块声明，供 GUI 和 HTTP 服务共享

// 核心层（UIA + 数据模型）
pub mod core;

// 公共便捷导出
pub use core::*;

// API 层
pub mod api;

// Capture 包装层（供 GUI 和 API 使用）
pub mod capture;

// 鼠标控制（GUI + HTTP 共享）
pub mod mouse_control;