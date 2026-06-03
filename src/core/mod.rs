// src/core/mod.rs
//
// 核心层 - UI Automation 封装和数据模型
// GUI 应用和 HTTP 服务共享此模块

pub mod model;
pub mod xpath;
pub mod xpath_optimizer;
pub mod error;
pub mod uia;
pub mod uia_context;
pub mod element_cache;
pub mod enum_windows;
pub mod screenshot;
pub mod commonality;
pub mod narrator;
pub mod metrics;

// Re-export commonly used types for convenience
pub use model::*;
pub use xpath::{generate, lint};
pub use xpath_optimizer::{XPathOptimizer, OptimizationResult, OptimizationSummary};
pub use error::{SelectorError, Result};  // NOTE: SelectorError is currently unused — kept for future use
pub use screenshot::{capture_region, save_screenshot, generate_screenshot_filename, get_default_screenshot_dir, normalize_rect, clamp_rect_to_screen, is_valid_rect};
pub use commonality::extract_common_path;

// Re-export fast window enumeration
pub use enum_windows::enumerate_windows_fast;

// Re-export uia_context public API
#[allow(deprecated)]
pub use uia_context::{init_uia_context, get_automation, with_automation, ensure_mta};
pub use element_cache::{cache_element, get_cached_element, cache_size as element_cache_size, clear_cache as clear_element_cache};