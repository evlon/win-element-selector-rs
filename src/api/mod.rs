// src/api/mod.rs
//
// HTTP API 模块 - 提供 REST 接口

pub mod types;
pub mod window;
pub mod element;
pub mod mouse;
pub mod idle_motion;
pub mod keyboard;

// 重新导出类型
pub use types::*;
pub use idle_motion::{with_auto_pause, IDLE_STATE};