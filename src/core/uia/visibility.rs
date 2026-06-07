// src/core/uia/visibility.rs
//
// Element visibility computation — pure functions for testing.
//
// All functions return core::model::VisibilityResult (no api layer dependency).

use crate::core::model::{OverflowInfo, Rect, VisibilityResult};
use uiautomation::core::UIElement;

/// Compute visibility of an element within a viewport and optional container.
///
/// This is a pure function: given the element's rect, viewport rect, and optional
/// container rect, it computes overflow, visibility status, position, and scroll direction.
pub fn compute_visibility(
    element_rect: &Rect,
    viewport_rect: &Rect,
    container_rect: Option<&Rect>,
    is_offscreen: Option<bool>,
) -> VisibilityResult {
    let intersect = |a: &Rect, b: &Rect| -> Option<Rect> {
        let left = a.x.max(b.x);
        let top = a.y.max(b.y);
        let right = (a.x + a.width).min(b.x + b.width);
        let bottom = (a.y + a.height).min(b.y + b.height);
        if right > left && bottom > top {
            Some(Rect { x: left, y: top, width: right - left, height: bottom - top })
        } else { None }
    };

    let clip_rect = container_rect
        .and_then(|cr| intersect(cr, viewport_rect))
        .or_else(|| Some(viewport_rect.clone()));

    let visible_rect = clip_rect.as_ref().and_then(|clip| intersect(element_rect, clip));

    let overflow_top = (viewport_rect.y - element_rect.y).max(0);
    let overflow_bottom = ((element_rect.y + element_rect.height) - (viewport_rect.y + viewport_rect.height)).max(0);
    let overflow_left = (viewport_rect.x - element_rect.x).max(0);
    let overflow_right = ((element_rect.x + element_rect.width) - (viewport_rect.x + viewport_rect.width)).max(0);
    let has_overflow = overflow_top > 0 || overflow_bottom > 0 || overflow_left > 0 || overflow_right > 0;

    let visibility = if !has_overflow { "fully_visible".to_string() }
        else if (overflow_top > 0 && overflow_bottom > 0) || (overflow_left > 0 && overflow_right > 0) { "offscreen".to_string() }
        else { "partially_visible".to_string() };

    let position = if !has_overflow { "inside".to_string() }
        else if overflow_top >= overflow_bottom && overflow_top >= overflow_left && overflow_top >= overflow_right { "above".to_string() }
        else if overflow_bottom >= overflow_top && overflow_bottom >= overflow_left && overflow_bottom >= overflow_right { "below".to_string() }
        else if overflow_left >= overflow_right { "left".to_string() }
        else { "right".to_string() };

    let scroll_direction = if !has_overflow { None }
        else if overflow_top > overflow_bottom { Some("down".to_string()) }
        else if overflow_bottom > overflow_top { Some("up".to_string()) }
        else if overflow_left > overflow_right { Some("right".to_string()) }
        else { Some("left".to_string()) };

    VisibilityResult {
        found: true,
        is_offscreen,
        visibility,
        position,
        element_rect: Some(element_rect.clone()),
        visible_rect,
        viewport_rect: Some(viewport_rect.clone()),
        overflow: Some(OverflowInfo { top: overflow_top, bottom: overflow_bottom, left: overflow_left, right: overflow_right }),
        scroll_direction,
        error: None,
    }
}

/// Build a "not found" visibility result.
pub fn visibility_not_found(error: &str) -> VisibilityResult {
    VisibilityResult {
        found: false, is_offscreen: None, visibility: "not_found".to_string(),
        position: "unknown".to_string(), element_rect: None, visible_rect: None,
        viewport_rect: None, overflow: None, scroll_direction: None,
        error: Some(error.to_string()),
    }
}

/// Build an "error" visibility result.
pub fn visibility_error(error: &str) -> VisibilityResult {
    VisibilityResult {
        found: false, is_offscreen: None, visibility: "error".to_string(),
        position: "unknown".to_string(), element_rect: None, visible_rect: None,
        viewport_rect: None, overflow: None, scroll_direction: None,
        error: Some(error.to_string()),
    }
}

/// Build a "found but no rect" visibility result.
pub fn visibility_no_rect(is_offscreen: Option<bool>, element_rect: Option<Rect>, error: &str) -> VisibilityResult {
    VisibilityResult {
        found: true, is_offscreen,
        visibility: if is_offscreen.unwrap_or(false) { "offscreen".to_string() } else { "visible".to_string() },
        position: "unknown".to_string(),
        element_rect, visible_rect: None, viewport_rect: None, overflow: None, scroll_direction: None,
        error: Some(error.to_string()),
    }
}

/// Get element visibility information (migrated from com_worker).
///
/// Uses `validate_xpath` to find the element,
/// then computes visibility within the viewport and optional container.
/// Returns `VisibilityResult` (core layer type, no api dependency).
pub fn get_element_visibility(
    window_selector: &str,
    element_xpath: &str,
    container_xpath: Option<&str>,
) -> VisibilityResult {
    use crate::core::model::ValidationResult;

    let detailed = super::validate_xpath(
        window_selector, element_xpath, &[], None, None, true,
    );

    let (element_rect, is_offscreen) = match &detailed.overall {
        ValidationResult::Found { first_rect, .. } => (first_rect.clone(), detailed.is_offscreen),
        ValidationResult::NotFound { .. } => return visibility_not_found("元素未找到"),
        ValidationResult::Error(e) => return visibility_error(e),
        _ => return visibility_not_found("校验状态未知"),
    };

    let elem_rect = match &element_rect {
        Some(r) => r,
        None => return visibility_no_rect(is_offscreen, None, "元素坐标获取失败"),
    };

    let window_rect = super::get_window_rect_by_selector(window_selector);
    let viewport_rect = match &window_rect {
        Some(r) => r,
        None => {
            let api_rect = Rect { x: elem_rect.x, y: elem_rect.y, width: elem_rect.width, height: elem_rect.height };
            return visibility_no_rect(is_offscreen, Some(api_rect), "窗口矩形获取失败");
        }
    };

    let elem_api_rect = Rect { x: elem_rect.x, y: elem_rect.y, width: elem_rect.width, height: elem_rect.height };
    let vp_api_rect = Rect { x: viewport_rect.x, y: viewport_rect.y, width: viewport_rect.width, height: viewport_rect.height };

    let container_api_rect = if let Some(cxpath) = container_xpath {
        let container_detailed = super::validate_xpath(window_selector, cxpath, &[], None, None, true);
        match &container_detailed.overall {
            ValidationResult::Found { first_rect: Some(cr), .. } => {
                Some(Rect { x: cr.x, y: cr.y, width: cr.width, height: cr.height })
            }
            _ => None,
        }
    } else {
        None
    };

    compute_visibility(&elem_api_rect, &vp_api_rect, container_api_rect.as_ref(), is_offscreen)
}

/// Get element visibility from a pre-resolved UIElement (no XPath search).
/// Used by the runtimeId cache path.
pub fn get_element_visibility_by_elem(
    elem: &UIElement,
    window_selector: &str,
    container_xpath: Option<&str>,
) -> VisibilityResult {
    // Read element rect from the cached UIElement
    let uia_rect = match elem.get_bounding_rectangle() {
        Ok(r) => r,
        Err(e) => return visibility_error(&format!("获取元素矩形失败: {}", e)),
    };
    let elem_rect = Rect {
        x: uia_rect.get_left(),
        y: uia_rect.get_top(),
        width: uia_rect.get_right() - uia_rect.get_left(),
        height: uia_rect.get_bottom() - uia_rect.get_top(),
    };

    let is_offscreen = elem.is_offscreen().ok();

    let window_rect = super::get_window_rect_by_selector(window_selector);
    let viewport_rect = match &window_rect {
        Some(r) => r,
        None => {
            return visibility_no_rect(is_offscreen, Some(elem_rect), "窗口矩形获取失败");
        }
    };
    let vp_api_rect = Rect {
        x: viewport_rect.x,
        y: viewport_rect.y,
        width: viewport_rect.width,
        height: viewport_rect.height,
    };

    let container_api_rect = if let Some(cxpath) = container_xpath {
        use crate::core::model::ValidationResult;
        let container_detailed = super::validate_xpath(window_selector, cxpath, &[], None, None, true);
        match &container_detailed.overall {
            ValidationResult::Found { first_rect: Some(cr), .. } => {
                Some(Rect { x: cr.x, y: cr.y, width: cr.width, height: cr.height })
            }
            _ => None,
        }
    } else {
        None
    };

    compute_visibility(&elem_rect, &vp_api_rect, container_api_rect.as_ref(), is_offscreen)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, width: w, height: h }
    }

    #[test]
    fn test_fully_visible() {
        let result = compute_visibility(&rect(10, 10, 50, 50), &rect(0, 0, 800, 600), None, None);
        assert_eq!(result.visibility, "fully_visible");
        assert_eq!(result.position, "inside");
        assert!(result.scroll_direction.is_none());
        assert!(result.visible_rect.is_some());
    }

    #[test]
    fn test_partially_visible_below() {
        let result = compute_visibility(&rect(10, 590, 50, 50), &rect(0, 0, 800, 600), None, None);
        assert_eq!(result.visibility, "partially_visible");
        assert_eq!(result.position, "below");
        assert_eq!(result.scroll_direction.as_deref(), Some("up"));
    }

    #[test]
    fn test_partially_visible_above() {
        let result = compute_visibility(&rect(10, -10, 50, 50), &rect(0, 0, 800, 600), None, None);
        assert_eq!(result.visibility, "partially_visible");
        assert_eq!(result.position, "above");
        assert_eq!(result.scroll_direction.as_deref(), Some("down"));
    }

    #[test]
    fn test_offscreen_both_directions() {
        let result = compute_visibility(&rect(-100, -100, 50, 50), &rect(0, 0, 800, 600), None, None);
        assert_eq!(result.visibility, "partially_visible");
    }

    #[test]
    fn test_no_overlap_bottom_right() {
        let result = compute_visibility(&rect(800, 600, 50, 50), &rect(0, 0, 800, 600), None, None);
        assert_eq!(result.visibility, "partially_visible");
    }

    #[test]
    fn test_with_container_clip() {
        let container = rect(100, 100, 200, 200);
        let result = compute_visibility(&rect(150, 150, 100, 100), &rect(0, 0, 800, 600), Some(&container), None);
        assert_eq!(result.visibility, "fully_visible");
    }
}
