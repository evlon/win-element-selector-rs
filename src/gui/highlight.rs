// src/gui/highlight.rs
//
// 元素高亮显示 - 薄包装层，从库级别 highlight 模块 re-export
// GUI 层直接使用共享实现，避免代码重复

#[allow(unused_imports)]
pub use element_selector::highlight::{
    flash, flash_with_info, flash_point, hide, update_highlight,
    show, show_with_info, HighlightHandle,
};
