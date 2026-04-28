// src/core/mod.rs
//
// 核心层 - UI Automation 封装和数据模型
// GUI 应用和 HTTP 服务共享此模块

pub mod model;
pub mod xpath;
pub mod xpath_optimizer;
pub mod error;
pub mod uia;
pub mod enum_windows;

// Re-export commonly used types for convenience
pub use model::*;
pub use xpath::{generate, lint};
pub use xpath_optimizer::{XPathOptimizer, OptimizationResult, OptimizationSummary, ClassStrategy, NameStrategy};
pub use error::{SelectorError, Result};

// Re-export fast window enumeration
#[cfg(target_os = "windows")]
pub use enum_windows::enumerate_windows_fast;