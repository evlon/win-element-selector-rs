// src/core/com_worker.rs
//
// COM Worker compatibility shim — bridges old API to new direct-call architecture.
//
// This module provides the same public API as the old COM Worker,
// but now directly calls uia.rs functions using the global MTA UIAutomation.
// No STA thread, no mpsc channel, no timeouts.
//
// Type conversion: UIAutomation → IUIAutomation via .as_ref(),
// UIElement → IUIAutomationElement via .clone().into()

use crate::core::model::{CaptureResult, DetailedValidationResult};
use crate::api::types::ElementInfo;
use crate::core::uia::InspectResult;
use uiautomation::core::{UIAutomation, UIElement};
use windows::Win32::UI::Accessibility::{IUIAutomation, IUIAutomationElement};

/// Helper: convert &UIAutomation to IUIAutomation (clone the inner COM interface)
fn auto_to_raw(auto: &UIAutomation) -> IUIAutomation {
    auto.as_ref().clone()
}

/// Helper: convert &UIElement to IUIAutomationElement (clone the inner COM interface)
fn elem_to_raw(elem: &UIElement) -> IUIAutomationElement {
    let raw: &IUIAutomationElement = elem.as_ref();
    raw.clone()
}

/// Helper: convert IUIAutomationElement to UIElement
fn raw_into_elem(elem: IUIAutomationElement) -> UIElement {
    UIElement::from(elem)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Public API — direct calls replacing channel-based communication
// ═══════════════════════════════════════════════════════════════════════════════

pub fn global_capture_at(_request_id: u64, x: i32, y: i32) -> anyhow::Result<CaptureResult> {
    Ok(crate::core::uia::capture_at_point(x, y))
}

pub fn global_capture_enhanced_at(_request_id: u64, x: i32, y: i32) -> anyhow::Result<CaptureResult> {
    Ok(crate::core::uia::capture_enhanced_at_point(x, y))
}

pub fn global_find_element(
    _request_id: u64,
    window_selector: String,
    xpath: String,
    random_range: Option<f32>,
) -> anyhow::Result<Vec<ElementInfo>> {
    use crate::core::uia::windows_impl;
    use crate::core::uia_context::get_automation;
    use crate::core::element_cache::cache_element;

    let auto = get_automation();
    let raw_auto = auto_to_raw(auto);

    let windows = windows_impl::find_window_by_selector_public(&raw_auto, &window_selector);
    if windows.is_empty() {
        return Ok(vec![]);
    }

    let random_range_val = random_range.unwrap_or(5.0);
    let mut all_results: Vec<ElementInfo> = Vec::new();

    for window in &windows {
        let window_rect = unsafe {
            window.CurrentBoundingRectangle().ok().map(|r| {
                crate::api::types::Rect {
                    x: r.left,
                    y: r.top,
                    width: r.right - r.left,
                    height: r.bottom - r.top,
                }
            })
        };

        let (elements, _) = match windows_impl::find_by_xpath_with_fallback_public(&raw_auto, window, &xpath) {
            Ok(r) => r,
            Err(_) => continue,
        };

        if !elements.is_empty() {
            let mut rng = rand::thread_rng();
            for elem in &elements {
                if let Some(rid_str) = windows_impl::runtime_id_key_public(elem) {
                    cache_element(rid_str, raw_into_elem(elem.clone()));
                }
                if let Some(info) = windows_impl::element_info_from_uia_public(elem, window_rect.as_ref(), random_range_val, &mut rng) {
                    all_results.push(info);
                }
            }
            if !all_results.is_empty() {
                break;
            }
        }
    }

    Ok(all_results)
}

pub fn global_validate_xpath(
    _request_id: u64,
    window_selector: String,
    element_xpath: String,
    hierarchy: Vec<crate::core::model::HierarchyNode>,
) -> anyhow::Result<DetailedValidationResult> {
    Ok(crate::core::uia::validate_selector_and_xpath_detailed(
        &window_selector,
        &element_xpath,
        &hierarchy,
    ))
}

pub fn global_find_common_elements(
    _request_id: u64,
    _window_selector: String,
    xpath: String,
) -> anyhow::Result<Vec<crate::api::types::ElementInfo>> {
    let results = crate::core::uia::find_all_elements_from_root(&xpath, 5.0);
    Ok(results)
}

pub fn global_exists_window(_request_id: u64, window_selector: String) -> anyhow::Result<bool> {
    Ok(crate::core::uia::exists_window_by_selector(&window_selector))
}

pub fn global_activate_window(_request_id: u64, window_selector: String) -> anyhow::Result<bool> {
    Ok(crate::core::uia::activate_window_by_selector(&window_selector))
}

pub fn global_activate_and_focus_element(_request_id: u64, window_selector: String, xpath: String) -> anyhow::Result<bool> {
    Ok(crate::core::uia::activate_and_focus_element(&window_selector, &xpath))
}

pub fn global_list_windows(_request_id: u64) -> anyhow::Result<Vec<crate::core::model::WindowInfo>> {
    Ok(crate::capture::list_windows())
}

pub fn global_get_element_visibility(
    _request_id: u64,
    window_selector: String,
    element_xpath: String,
    container_xpath: Option<String>,
) -> anyhow::Result<crate::api::types::ElementVisibilityResponse> {
    use crate::api::types::Rect;
    use crate::core::model::ValidationResult;
    use crate::core::uia::{compute_visibility, visibility_not_found, visibility_error, visibility_no_rect};

    let detailed = crate::core::uia::validate_selector_and_xpath_detailed(
        &window_selector, &element_xpath, &[],
    );

    let (element_rect, is_offscreen) = match &detailed.overall {
        ValidationResult::Found { first_rect, .. } => (first_rect.clone(), detailed.is_offscreen),
        ValidationResult::NotFound => return Ok(visibility_not_found("元素未找到")),
        ValidationResult::Error(e) => return Ok(visibility_error(e)),
        _ => return Ok(visibility_not_found("校验状态未知")),
    };

    let elem_rect = match &element_rect {
        Some(r) => r,
        None => return Ok(visibility_no_rect(is_offscreen, None, "元素坐标获取失败")),
    };

    let window_rect = crate::core::uia::get_window_rect_by_selector(&window_selector);
    let viewport_rect = match &window_rect {
        Some(r) => r,
        None => {
            let api_rect = Rect { x: elem_rect.x, y: elem_rect.y, width: elem_rect.width, height: elem_rect.height };
            return Ok(visibility_no_rect(is_offscreen, Some(api_rect), "窗口矩形获取失败"));
        }
    };

    let elem_api_rect = Rect { x: elem_rect.x, y: elem_rect.y, width: elem_rect.width, height: elem_rect.height };
    let vp_api_rect = Rect { x: viewport_rect.x, y: viewport_rect.y, width: viewport_rect.width, height: viewport_rect.height };

    let container_api_rect = if let Some(cxpath) = container_xpath {
        let container_detailed = crate::core::uia::validate_selector_and_xpath_detailed(&window_selector, &cxpath, &[]);
        match &container_detailed.overall {
            ValidationResult::Found { first_rect: Some(cr), .. } => {
                Some(Rect { x: cr.x, y: cr.y, width: cr.width, height: cr.height })
            }
            _ => None,
        }
    } else {
        None
    };

    Ok(compute_visibility(&elem_api_rect, &vp_api_rect, container_api_rect.as_ref(), is_offscreen))
}

pub fn global_get_element_rect_at_point(_request_id: u64, x: i32, y: i32) -> anyhow::Result<Option<crate::core::model::ElementRect>> {
    use crate::core::uia_context::get_automation;
    let auto = get_automation();
    let raw_auto = auto_to_raw(auto);
    let pt = windows::Win32::Foundation::POINT { x, y };
    let element: IUIAutomationElement = unsafe {
        match raw_auto.ElementFromPoint(pt) {
            Ok(e) => e,
            Err(_) => return Ok(None),
        }
    };
    match unsafe { element.CurrentBoundingRectangle() } {
        Ok(r) => Ok(Some(crate::core::model::ElementRect {
            x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top,
        })),
        Err(_) => Ok(None),
    }
}

pub fn global_inspect(
    _request_id: u64,
    window_selector: String,
    element_xpath: String,
    max_depth: usize,
    max_nodes: usize,
    format: String,
) -> anyhow::Result<InspectResult> {
    Ok(crate::core::uia::inspect_subtree(&window_selector, &element_xpath, max_depth, max_nodes, &format))
}

pub fn global_navigate(
    _request_id: u64,
    window_selector: String,
    base_xpath: String,
    steps: Vec<crate::api::types::NavigateStep>,
) -> anyhow::Result<Result<(Option<crate::api::types::ElementInfo>, String), String>> {
    Ok(crate::core::uia::navigate_from_element(&window_selector, &base_xpath, &steps))
}

pub fn global_find_from_element(
    _request_id: u64,
    runtime_id: String,
    xpath: String,
    random_range: f32,
) -> anyhow::Result<Vec<crate::api::types::ElementInfo>> {
    use crate::core::uia::windows_impl;
    use crate::core::uia_context::get_automation;
    use crate::core::element_cache::{cache_element, get_cached_element};

    let auto = get_automation();
    let raw_auto = auto_to_raw(auto);

    let base_elem = match get_cached_element(&runtime_id) {
        Some(e) => elem_to_raw(&e),
        None => {
            log::warn!("[find_from_element] Element not found in cache: runtime_id={}", runtime_id);
            return Ok(vec![]);
        }
    };

    let results = windows_impl::find_from_element_impl(&raw_auto, &base_elem, &xpath, random_range);

    // Cache found elements for subsequent lookups
    if let Ok((raw_elements, _)) = windows_impl::find_by_xpath_detailed_public(&raw_auto, &base_elem, &xpath) {
        for raw_elem in &raw_elements {
            if let Some(rid_str) = windows_impl::runtime_id_key_public(raw_elem) {
                cache_element(rid_str, raw_into_elem(raw_elem.clone()));
            }
        }
    }

    Ok(results)
}
