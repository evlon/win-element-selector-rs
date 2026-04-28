// src/api/mod.rs
//
// HTTP API 模块 - 提供 REST 接口

pub mod types;
pub mod window;
pub mod element;
pub mod mouse;

// 重新导出类型
pub use types::*;