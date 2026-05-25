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
pub mod com_worker;
pub mod similarity;
pub mod screenshot;
pub mod commonality;
pub mod narrator;

// Re-export commonly used types for convenience
pub use model::*;
pub use xpath::{generate, lint};
pub use xpath_optimizer::{XPathOptimizer, OptimizationResult, OptimizationSummary};
pub use error::{SelectorError, Result};
pub use similarity::{bounds_similarity, children_structure_similarity, calculate_overall_similarity, is_similar, SimilarElementSample, ChildFeature, RelativeRect};
pub use screenshot::{capture_region, save_screenshot, generate_screenshot_filename, get_default_screenshot_dir, normalize_rect, clamp_rect_to_screen, is_valid_rect};
pub use commonality::extract_common_path;

// Re-export fast window enumeration
pub use enum_windows::enumerate_windows_fast;