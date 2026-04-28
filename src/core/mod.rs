// src/core/mod.rs
//
// 核心层 - UI Automation 封装和数据模型
// GUI 应用和 HTTP 服务共享此模块

pub mod model;
pub mod xpath;
pub mod error;
pub mod uia;

// Re-export commonly used types for convenience
pub use model::*;
pub use xpath::{generate, lint};
pub use error::{SelectorError, Result};