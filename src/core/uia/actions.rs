// src/core/uia/actions.rs
//
// Element resolution helpers for action functions (click, hover, type, etc.).
// Provides runtimeId-first caching with no implicit XPath fallback.

use anyhow;
use log::warn;
use uiautomation::core::UIElement;

/// Resolve an element for action operations.
///
/// **Strategy**: runtimeId → cache lookup only (no XPath fallback).
/// - Cache hit + element valid → Ok(UIElement)
/// - Cache miss or expired → Err (caller should decide fallback)
/// - Element invalid (stale COM reference) → Err + remove from cache
pub fn resolve_element_for_action(
    runtime_id: &str,
    window_selector: &str,
    element_xpath: &str,
) -> anyhow::Result<UIElement> {
    match crate::core::element_cache::get_cached_element(runtime_id) {
        Some(elem) => {
            if is_element_valid(&elem) {
                Ok(elem)
            } else {
                // Stale element → remove from cache
                crate::core::element_cache::remove_cached_element(runtime_id);
                warn!(
                    "Cached element invalid (stale COM reference): runtimeId={}, xpath={}",
                    runtime_id, element_xpath
                );
                anyhow::bail!(
                    "缓存元素已失效 (stale): runtimeId={}, xpath={}, window={}",
                    runtime_id,
                    element_xpath,
                    window_selector
                )
            }
        }
        None => {
            anyhow::bail!(
                "元素不在缓存中: runtimeId={}, xpath={}, window={}",
                runtime_id,
                element_xpath,
                window_selector
            )
        }
    }
}

/// Lightweight check whether a cached UIElement is still valid.
/// Attempts a cheap COM property read to verify the reference is alive.
fn is_element_valid(elem: &UIElement) -> bool {
    // Try two lightweight property reads; either succeeding means the element is alive.
    elem.get_control_type().is_ok() || elem.get_name().is_ok()
}
