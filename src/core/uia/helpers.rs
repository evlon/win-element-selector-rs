use super::*;
use crate::core::model::{CaptureMode, CaptureResult, ElementRect, HierarchyNode, WalkerHint, WindowInfo};
use uiauto_xpath::control_type_id_to_name;
use uiautomation::types::Point as UiaPoint;
use uiautomation::types::ControlType;

pub(super) const WEBVIEW_CLASS_PREFIXES: &[&str] = &[
    "WRY_WEBVIEW",           // Tauri/WRY apps
    "Chrome_WidgetWin",      // Chromium-based (Electron, Edge, etc.)
    "Chrome_Widget",         // Shorter prefix for Chrome_WidgetWin_1, etc.
    "Intermediate D3D",      // D3D intermediate window (some WRY versions)
    "WebView2",              // WebView2 apps
    "CefBrowserWindow",      // CEF-based apps
    "QtWebEngine",           // Qt's WebEngine
    "QWebEngine",            // Qt WebEngine variant
];

pub(super) const RENDERING_LAYER_CLASSES: &[&str] = &[
    "MMUIRenderSubWindowHW",  // WeChat's hardware rendering surface (FrameworkId='Win32')
    "Intermediate D3D",       // D3D intermediate window
];

pub(super) fn is_webview_class(class_name: &str) -> bool {
    WEBVIEW_CLASS_PREFIXES.iter().any(|prefix| class_name.starts_with(prefix))
}

pub(super) fn determine_walker_hint(
    class_name: &str,
    framework_id: &str,
    node_pid: u32,
    window_pid: u32,
) -> WalkerHint {
    use crate::core::model::WalkerHint;

    // 1. Cross-process WebView: PID differs from window → ChildHwnd
    if node_pid != 0 && window_pid != 0 && node_pid != window_pid {
        // But only if it's actually a WebView class
        if is_webview_class(class_name) {
            return WalkerHint::ChildHwnd;
        }
    }

    // 2. WebView class (even in same process) → RawView
    // These elements are typically filtered from ControlView but visible in RawView
    if is_webview_class(class_name) {
        return WalkerHint::RawView;
    }

    // 3. FrameworkId mismatch hints at embedded content
    // Chrome framework inside Win32 window → RawView may be needed
    if !framework_id.is_empty() {
        if framework_id.eq_ignore_ascii_case("Chrome") || framework_id.eq_ignore_ascii_case("WebView") {
            return WalkerHint::RawView;
        }
    }

    // 4. Default: ControlView (fastest, works for most native controls)
    WalkerHint::ControlView
}

pub(super) fn set_walker_hints(hierarchy: &mut [HierarchyNode], window_pid: u32) {
    for node in hierarchy.iter_mut() {
        if node.walker_hint == WalkerHint::Unknown {
            node.walker_hint = determine_walker_hint(
                &node.class_name,
                &node.framework_id,
                node.process_id,
                window_pid,
            );
        }
    }
}

pub(super) fn is_rendering_layer(class_name: &str) -> bool {
    RENDERING_LAYER_CLASSES.iter().any(|c| class_name == *c)
}

/// Simple rect for point-in-rect checks, decoupled from windows-rs RECT and uiautomation-rs Rect.
#[derive(Debug, Clone, Copy)]
pub(super) struct SimpleRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl SimpleRect {
    pub fn width(&self) -> i32 { self.right - self.left }
    pub fn height(&self) -> i32 { self.bottom - self.top }
}

impl From<&UiaRect> for SimpleRect {
    fn from(r: &UiaRect) -> Self {
        SimpleRect { left: r.get_left(), top: r.get_top(), right: r.get_right(), bottom: r.get_bottom() }
    }
}

impl From<windows::Win32::Foundation::RECT> for SimpleRect {
    fn from(r: windows::Win32::Foundation::RECT) -> Self {
        SimpleRect { left: r.left, top: r.top, right: r.right, bottom: r.bottom }
    }
}

pub(super) fn point_in_rect(x: i32, y: i32, r: &SimpleRect) -> bool {
    x >= r.left && x <= r.right && y >= r.top && y <= r.bottom
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // fields used in tests and for diagnostic logging
pub(super) struct CandidateElement {
    /// index in the FindAll result array (later = deeper in tree traversal)
    index: i32,
    /// BoundingRectangle area (width * height)
    area: i64,
    /// Control type name (e.g. "Text", "Group")
    control_type: String,
    /// Element name
    name: String,
    /// RuntimeId (for dedup; NOT used for selection ordering)
    runtime_id: Vec<i32>,
}

pub(super) fn is_leaf_control_type(ct: &str) -> bool {
    matches!(ct,
        "Text" | "Button" | "Hyperlink" | "Edit" | "CheckBox" | "RadioButton"
        | "ComboBox" | "ListItem" | "TreeItem" | "TabItem" | "MenuItem"
        | "DataItem" | "Image" | "ScrollBar" | "Slider" | "Spinner"
        | "ProgressBar" | "Thumb"
    )
}

#[cfg(test)]
pub(super) fn candidate_dominates_findall(
    candidate: &CandidateElement,
    best_area: i64,
    best_index: i32,
    best_ct: &str,
    best_name: &str,
) -> bool {
    let c_is_leaf = is_leaf_control_type(&candidate.control_type);
    let b_is_leaf = is_leaf_control_type(best_ct);
    let c_has_name = !candidate.name.is_empty();
    let b_has_name = !best_name.is_empty();

    // Leaf preference: a leaf element with a meaningful name always dominates
    // a container element with an empty name, regardless of area.
    // This fixes: Group(name='', area=200) beating Text(name='article title', area=30000)
    if c_is_leaf && c_has_name && !b_is_leaf && !b_has_name {
        return true;
    }
    // Reverse: container without name does NOT dominate leaf with name
    if !c_is_leaf && !c_has_name && b_is_leaf && b_has_name {
        return false;
    }

    // Default: smallest area wins, with later index as tiebreaker
    candidate.area < best_area
        || (candidate.area == best_area && candidate.index > best_index)
}

#[cfg(test)]
pub(super) fn candidate_dominates_drilldown(child_area: i64, child_rid_len: usize, deeper_area: i64, deeper_rid_len: usize) -> bool {
    child_area < deeper_area
        || (child_area == deeper_area && child_rid_len > deeper_rid_len)
}

pub(super) fn compute_element_visible_rect(
    element_rect: &crate::core::model::Rect,
    window_rect: Option<&crate::core::model::Rect>,
) -> Option<crate::core::model::Rect> {
    match window_rect {
        Some(vp) => {
            let left = element_rect.x.max(vp.x);
            let top = element_rect.y.max(vp.y);
            let right = (element_rect.x + element_rect.width).min(vp.x + vp.width);
            let bottom = (element_rect.y + element_rect.height).min(vp.y + vp.height);
            if right > left && bottom > top {
                Some(crate::core::model::Rect { 
                    x: left, 
                    y: top, 
                    width: right - left, 
                    height: bottom - top 
                })
            } else {
                None  // 完全不可见
            }
        },
        None => Some(element_rect.clone()),  // 无窗口信息时，认为整个元素可见
    }
}

pub(super) fn element_info_from_uia<R: rand::Rng>(
    elem: &UIElement,
    container_rect: Option<&crate::core::model::Rect>,
    random_range: f32,
    rng: &mut R,
) -> Option<crate::core::model::ElementData> {
    use crate::core::model::{Rect, Point, ElementData};

    let uia_rect = match elem.get_bounding_rectangle() {
        Ok(r) => r,
        Err(_) => return None,
    };
    let api_rect = Rect {
        x: uia_rect.get_left(),
        y: uia_rect.get_top(),
        width: uia_rect.get_right() - uia_rect.get_left(),
        height: uia_rect.get_bottom() - uia_rect.get_top(),
    };
    let center = api_rect.center();

    // 计算 visibleRect
    let visible_rect = compute_element_visible_rect(&api_rect, container_rect);

    // Calculate random center
    let half_range_w = api_rect.width as f32 * random_range / 2.0;
    let half_range_h = api_rect.height as f32 * random_range / 2.0;

    // 防止空范围导致 panic
    let offset_x = if half_range_w > 0.0 {
        rng.gen_range(-half_range_w..half_range_w) as i32
    } else {
        0
    };
    let offset_y = if half_range_h > 0.0 {
        rng.gen_range(-half_range_h..half_range_h) as i32
    } else {
        0
    };
    let center_random = Point::new(center.x + offset_x, center.y + offset_y);

    let is_offscreen = elem.is_offscreen().unwrap_or(false);

    // rect 始终保留：即使元素标记为 offscreen，坐标仍有效（如副屏负坐标场景）
    // center / center_random 仅在非 offscreen 时提供，避免误点击不可见元素
    let (center_opt, cr_opt) = if is_offscreen {
        (None, None)
    } else {
        (Some(center), Some(center_random))
    };

    Some(ElementData {
        rect: Some(api_rect),
        visible_rect,
        center: center_opt,
        center_random: cr_opt,
        control_type: elem.get_control_type_raw().map(control_type_name).unwrap_or_default(),
        name: elem.get_name().unwrap_or_default(),
        automation_id: elem.get_automation_id().unwrap_or_default(),
        class_name: elem.get_classname().unwrap_or_default(),
        framework_id: elem.get_framework_id().unwrap_or_default(),
        help_text: elem.get_help_text().unwrap_or_default(),
        localized_control_type: elem.get_localized_control_type().unwrap_or_default(),
        is_enabled: elem.is_enabled().unwrap_or(true),
        is_offscreen,
        is_password: elem.is_password().unwrap_or(false),
        accelerator_key: elem.get_accelerator_key().unwrap_or_default(),
        access_key: elem.get_access_key().unwrap_or_default(),
        item_type: elem.get_item_type().unwrap_or_default(),
        item_status: elem.get_item_status().unwrap_or_default(),
        process_id: elem.get_process_id().unwrap_or(0),
        runtime_id: elem.get_runtime_id().ok().map(|ids| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",")),
        is_checkable: None,
        is_checked: None,
        is_clickable: None,
        is_scrollable: None,
        is_selected: None,
    })
}

pub(super) fn runtime_id_key(elem: &UIElement) -> Option<Vec<i32>> {
    elem.get_runtime_id().ok()
}

pub enum ComApartmentType {
    /// 未初始化
    Uninitialized,
    /// STA (Single-Threaded Apartment) - UIA 需要
    Sta,
    /// MTA (Multi-Threaded Apartment) - 不兼容 UIA
    Mta,
}

pub struct AutomationProvider;

impl AutomationProvider {
    /// 获取 UIAutomation 实例，带健康检查
    pub fn get_healthy() -> anyhow::Result<UIAutomation> {
        AUTOMATION.with(|cell| {
            let mut opt = cell.borrow_mut();
            
            // 检查现有实例是否有效
            if let Some(ref auto) = *opt {
                // 尝试一个简单的操作来验证实例是否仍然有效
                if Self::validate_instance(auto) {
                    log::debug!("Reusing existing UIAutomation instance");
                    return Ok(auto.clone());
                } else {
                    log::warn!("Existing UIAutomation instance is invalid, recreating...");
                    *opt = None;
                }
            }
            
            // 创建新实例（使用 new_direct，因为 MTA 已在 uia_context 中初始化）
            log::debug!("Creating new UIAutomation instance");
            let auto = UIAutomation::new_direct()
                .map_err(|e| anyhow::anyhow!("UIAutomation::new_direct: {e}"))?;
            
            *opt = Some(auto.clone());
            Ok(auto)
        })
    }
    
    /// 验证 UIAutomation 实例是否有效
    fn validate_instance(auto: &UIAutomation) -> bool {
        use std::time::Instant;
        
        // 尝试获取根元素作为健康检查，并设置超时
        let start = Instant::now();
        let result = auto.get_root_element();
        let elapsed = start.elapsed();
        
        // 如果操作超过 100ms，认为 COM 对象已经失效
        if elapsed.as_millis() > 100 {
            log::warn!("UIAutomation health check took {}ms (too slow, likely stale)", 
                      elapsed.as_millis());
            return false;
        }
        
        result.is_ok()
    }
    
    /// 强制重置 UIAutomation 实例
    pub fn force_reset() {
        AUTOMATION.with(|cell| {
            let mut opt = cell.borrow_mut();
            *opt = None;
            log::debug!("UIAutomation instance reset");
        });
    }
}

thread_local! {
    static AUTOMATION: std::cell::RefCell<Option<UIAutomation>> =
        std::cell::RefCell::new(None);
}

pub fn get_automation() -> anyhow::Result<UIAutomation> {
    AutomationProvider::get_healthy()
}

/// Get element rect at screen coordinate (migrated from com_worker).
///
/// Uses ElementFromPoint to find the topmost element at the given point,
/// then returns its bounding rectangle.
pub fn get_element_rect_at_point(x: i32, y: i32) -> Option<crate::core::model::ElementRect> {
    let auto = match get_automation() {
        Ok(a) => a,
        Err(_) => return None,
    };
    let point = UiaPoint::new(x, y);
    let element = match auto.element_from_point(point) {
        Ok(e) => e,
        Err(_) => return None,
    };
    match element.get_bounding_rectangle() {
        Ok(r) => Some(crate::core::model::ElementRect {
            x: r.get_left(), y: r.get_top(),
            width: r.get_right() - r.get_left(),
            height: r.get_bottom() - r.get_top(),
        }),
        Err(_) => None,
    }
}

pub fn capture_at_cursor() -> CaptureResult {
    let pt = unsafe {
        let mut p = POINT::default();
        if GetCursorPos(&mut p).is_err() {
            return CaptureResult {
                hierarchy: vec![],
                cursor_x: 0, cursor_y: 0,
                error: Some("GetCursorPos 失败".to_string()),
                window_info: None,
                capture_mode: CaptureMode::Fast,
            };
        }
        p
    };
    crate::core::uia::capture_at_point(pt.x, pt.y)
}

pub(super) fn get_process_name_by_id(process_id: u32) -> String {
    use windows::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    unsafe {
        let handle_result = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id);
        let handle = match handle_result {
            Ok(h) => h,
            Err(_) => return String::new(),
        };
        
        if handle == HANDLE::default() {
            return String::new();
        }

        let mut buffer = [0u16; 260]; // MAX_PATH
        let mut length = buffer.len() as u32;
        
        let result = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buffer.as_mut_ptr()),
            &mut length,
        );
        
        let _ = CloseHandle(handle);
        
        if result.is_ok() && length > 0 {
            // Get just the filename without path
            let full_path = OsString::from_wide(&buffer[..length as usize]);
            if let Some(path) = full_path.to_str() {
                if let Some(filename) = path.rsplit('\\').next() {
                    // Remove .exe extension
                    return filename.strip_suffix(".exe")
                        .unwrap_or(filename)
                        .to_string();
                }
            }
        }
    }
    
    String::new()
}

pub(super) fn extract_window_info(hierarchy: &[HierarchyNode]) -> Option<WindowInfo> {
    if hierarchy.len() < 2 {
        return None;
    }
    
    // 顶层窗口 = Desktop（hierarchy[0]）的第一个非桌面子节点
    // 这必须是 hierarchy[1]，因为向上遍历是从目标到Desktop
    // Desktop之后的第一个节点就是顶层窗口
    let top_window = hierarchy.iter()
        .skip(1) // 跳过 Desktop
        .next()?; // 取第一个 = 顶层窗口
    
    // 获取进程名
    let process_name = get_process_name_by_id(top_window.process_id);
    
    Some(WindowInfo {
        title: top_window.name.clone(),
        class_name: top_window.class_name.clone(),
        process_id: top_window.process_id,
        process_name,
    })
}

pub(super) fn is_control_type_clickable(control_type: &str) -> bool {
    matches!(control_type, "Button" | "Hyperlink" | "ListItem" | "MenuItem"
        | "TabItem" | "TreeItem" | "RadioButton" | "CheckBox"
        | "ComboBox" | "Link" | "Image" | "Document")
}

pub(super) fn element_to_node(
    elem: &UIElement,
    _auto: &UIAutomation,
) -> Option<HierarchyNode> {
    let control_type = elem.get_control_type_raw()
        .map(control_type_name)
        .unwrap_or_default();

    let automation_id = elem.get_automation_id().unwrap_or_default();
    let class_name    = elem.get_classname().unwrap_or_default();
    let name          = elem.get_name().unwrap_or_default();
    let process_id    = elem.get_process_id().unwrap_or(0);
    
    // Extract extended properties
    let framework_id = elem.get_framework_id().unwrap_or_default();
    let help_text = elem.get_help_text().unwrap_or_default();
    let localized_control_type = elem.get_localized_control_type().unwrap_or_default();
    let is_enabled = elem.is_enabled().unwrap_or(true);
    let is_offscreen = elem.is_offscreen().unwrap_or(false);
    let is_password = elem.is_password().unwrap_or(false);
    
    // AccRole is deprecated in UIA, use ControlType instead
    let acc_role = String::new();

    let rect = elem.get_bounding_rectangle()
        .map(|r| ElementRect {
            x:      r.get_left(),
            y:      r.get_top(),
            width:  r.get_right()  - r.get_left(),
            height: r.get_bottom() - r.get_top(),
        })
        .unwrap_or_default();

    debug!(
        "element: type={control_type} aid={automation_id} \
         class={class_name} name={name} pid={process_id} \
         framework={framework_id} enabled={is_enabled}"
    );

    let mut node = HierarchyNode::new(
        control_type.clone(), automation_id.clone(), class_name.clone(), name.clone(),
        0, rect, process_id,
    );
    
    // Fill extended properties
    node.framework_id = framework_id;
    node.acc_role = acc_role;
    node.help_text = help_text;
    node.localized_control_type = localized_control_type;
    node.is_enabled = is_enabled;
    node.is_offscreen = is_offscreen;
    node.is_password = is_password;
    node.accelerator_key = elem.get_accelerator_key().unwrap_or_default();
    node.access_key = elem.get_access_key().unwrap_or_default();
    node.item_type = elem.get_item_type().unwrap_or_default();
    node.item_status = elem.get_item_status().unwrap_or_default();

    // ─── UIA Pattern detection ───────────────────────────────────────────
    {
        use uiautomation::patterns::{UITogglePattern, UISelectionItemPattern, UIInvokePattern, UIScrollPattern};
        
        let has_toggle = elem.get_pattern::<UITogglePattern>().is_ok();
        let has_invoke = elem.get_pattern::<UIInvokePattern>().is_ok();
        let has_scroll = elem.get_pattern::<UIScrollPattern>().is_ok();
        let has_selection_item = elem.get_pattern::<UISelectionItemPattern>().is_ok();

        node.is_checkable = has_toggle;
        node.is_clickable = has_invoke || is_control_type_clickable(&control_type);
        node.is_scrollable = has_scroll;

        // Read ToggleState if checkable
        if has_toggle {
            if let Ok(toggle) = elem.get_pattern::<UITogglePattern>() {
                if let Ok(state) = toggle.get_toggle_state() {
                    // ToggleState_Off = 0, ToggleState_On = 1, ToggleState_Indeterminate = 2
                    node.is_checked = Some(matches!(state, uiautomation::types::ToggleState::On));
                }
            }
        }

        // Read SelectionItem IsSelected if available
        if has_selection_item {
            if let Ok(sel) = elem.get_pattern::<UISelectionItemPattern>() {
                if let Ok(selected) = sel.is_selected() {
                    node.is_selected = Some(selected);
                }
            }
        }
    }
    
    // Build extended property filters from all populated fields
    node.build_extended_filters();
    
    Some(node)
}

pub(super) fn sibling_index(
    target: &UIElement,
    walker: &UITreeWalker,
) -> Option<i32> {
    let parent = walker.get_parent(target).ok()?;
    let mut child = walker.get_first_child(&parent).ok()?;
    let target_ct = target.get_control_type_raw().ok()?;

    let mut idx = 0i32;
    loop {
        let ct = child.get_control_type_raw().ok()?;
        if ct == target_ct {
            idx += 1;
        }
        // Compare by AutomationId (same logic as before)
        let aid_child = child.get_automation_id().unwrap_or_default();
        let aid_target = target.get_automation_id().unwrap_or_default();
        if aid_child == aid_target { return Some(idx); }
        match walker.get_next_sibling(&child) {
            Ok(next) => child = next,
            Err(_)   => break,
        }
    }
    None
}

pub(super) fn count_siblings(
    target: &UIElement,
    walker: &UITreeWalker,
) -> Option<i32> {
    let parent = walker.get_parent(target).ok()?;
    let mut child = walker.get_first_child(&parent).ok()?;
    let target_ct = target.get_control_type_raw().ok()?;

    let mut count = 0i32;
    loop {
        let ct = child.get_control_type_raw().ok()?;
        if ct == target_ct {
            count += 1;
        }
        match walker.get_next_sibling(&child) {
            Ok(next) => child = next,
            Err(_)   => break,
        }
    }
    Some(count)
}

pub(super) fn control_type_name(id: i32) -> String {
    control_type_id_to_name(id).to_string()
}

/// Convert a uiautomation-rs ControlType to its string name
pub(super) fn control_type_name_from_enum(ct: &ControlType) -> String {
    control_type_id_to_name(*ct as i32).to_string()
}

pub fn enumerate_windows() -> Vec<WindowInfo> {
    let auto = match get_automation() {
        Ok(a) => a,
        Err(_) => return vec![],
    };

    let desktop = match auto.get_root_element() {
        Ok(d) => d,
        Err(_) => return vec![],
    };

    let condition = match auto.create_true_condition() {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let windows = match desktop.find_all(uiautomation::types::TreeScope::Descendants, &condition) {
        Ok(w) => w,
        Err(_) => return vec![],
    };

    debug!("enumerate_windows: found {} total elements (TreeScope_Descendants)", windows.len());

    let mut window_list = Vec::new();
    for win in &windows {
        let ct = win.get_control_type_raw()
            .map(|id| control_type_name(id))
            .unwrap_or_default();

        // 支持多种窗口类型：Window, Pane, Application
        let valid_window_types = ["Window", "Pane", "Application"];
        
        if valid_window_types.contains(&ct.as_str()) {
            let title = win.get_name().unwrap_or_default();
            let class = win.get_classname().unwrap_or_default();
            let pid = win.get_process_id().unwrap_or(0);

            // Only include windows with non-empty title
            if !title.is_empty() {
                // Get window rect for size checking
                let rect = win.get_bounding_rectangle()
                    .map(|r| (r.get_right() - r.get_left(), r.get_bottom() - r.get_top()))
                    .unwrap_or((0, 0));
                // Feature-based filtering
                if rect.0 < 100 || rect.1 < 100 {
                    continue;
                }
                
                let is_shell_window = class.starts_with("Shell") 
                    || class == "Progman" 
                    || class == "WorkerW";
                if is_shell_window {
                    continue;
                }
                
                let is_sub_component = class.contains("Host") 
                    || (class.contains("View") && !class.contains("Chrome_WidgetWin"))
                    || class.contains("List") 
                    || class.contains("Tab") 
                    || class.contains("Tip") 
                    || class.starts_with("Windows.UI") 
                    || class.starts_with("Microsoft.UI");
                
                if is_sub_component {
                    continue;
                }
                
                let process_name = get_process_name_by_id(pid);
                window_list.push(WindowInfo {
                    title,
                    class_name: class,
                    process_id: pid,
                    process_name,
                });
            }
        }
    }

    window_list
}

pub(super) fn build_relative_xpath(
    control_type: &str,
    name: &str,
    class_name: &str,
    automation_id: &str,
    parent_xpath: &str,
) -> String {
    let predicate = if !automation_id.is_empty() {
        format!("[@AutomationId='{}']", automation_id)
    } else if !class_name.is_empty() {
        format!("[@ClassName='{}']", class_name)
    } else if !name.is_empty() {
        format!("[@Name='{}']", name)
    } else {
        String::new()
    };

    if parent_xpath.is_empty() {
        format!("/{}{}", control_type, predicate)
    } else {
        format!("{}/{}{}", parent_xpath, control_type, predicate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── point_in_rect tests ─────────────────────────────────────────────

    #[test]
    fn test_point_in_rect_basic() {
        let rect = SimpleRect { left: 100, top: 200, right: 300, bottom: 400 };
        assert!(point_in_rect(100, 200, &rect), "top-left corner should be inclusive");
        assert!(point_in_rect(300, 400, &rect), "bottom-right corner should be inclusive");
        assert!(point_in_rect(200, 300, &rect), "center should be inside");
        assert!(!point_in_rect(99, 200, &rect), "just outside left");
        assert!(!point_in_rect(100, 199, &rect), "just outside top");
        assert!(!point_in_rect(301, 300, &rect), "just outside right");
        assert!(!point_in_rect(200, 401, &rect), "just outside bottom");
    }

    #[test]
    fn test_point_in_rect_single_pixel() {
        let rect = SimpleRect { left: 50, top: 50, right: 50, bottom: 50 };
        assert!(point_in_rect(50, 50, &rect), "single pixel point should match");
        assert!(!point_in_rect(49, 50, &rect));
        assert!(!point_in_rect(51, 50, &rect));
    }

    // ─── is_leaf_control_type tests ──────────────────────────────────────

    #[test]
    fn test_is_leaf_control_type() {
        assert!(is_leaf_control_type("Text"));
        assert!(is_leaf_control_type("Button"));
        assert!(is_leaf_control_type("Hyperlink"));
        assert!(is_leaf_control_type("Edit"));
        assert!(is_leaf_control_type("Image"));
        assert!(is_leaf_control_type("ListItem"));
        assert!(!is_leaf_control_type("Group"));
        assert!(!is_leaf_control_type("Pane"));
        assert!(!is_leaf_control_type("Window"));
        assert!(!is_leaf_control_type("Document"));
    }

    // ─── candidate_dominates_findall tests ───────────────────────────────

    pub(super) fn select_best_findall(candidates: &[CandidateElement]) -> Option<&CandidateElement> {
        let mut best: Option<&CandidateElement> = None;
        let mut best_area = i64::MAX;
        let mut best_index: i32 = -1;
        let mut best_ct = String::new();
        let mut best_name = String::new();
        for c in candidates {
            let ct = best_ct.clone();
            let nm = best_name.clone();
            if candidate_dominates_findall(c, best_area, best_index, &ct, &nm) {
                best = Some(c);
                best_area = c.area;
                best_index = c.index;
                best_ct = c.control_type.clone();
                best_name = c.name.clone();
            }
        }
        best
    }

    /// THE KEY FIX: Chrome Group (larger area, long rid) vs Text (smaller area, short rid).
    /// Old code (rid_len priority) selected Group. New code (area priority) selects Text.
    #[test]
    fn test_findall_area_beats_rid_len_chrome_text_vs_group() {
        let candidates = vec![
            CandidateElement {
                index: 5,
                area: 4000,
                control_type: "Group".into(),
                name: String::new(),
                runtime_id: vec![42, 7, 3, 9, 15, 20],
            },
            CandidateElement {
                index: 8,
                area: 2000,
                control_type: "Text".into(),
                name: "some text content".into(),
                runtime_id: vec![42, 7, 3],
            },
        ];
        let best = select_best_findall(&candidates).unwrap();
        assert_eq!(best.control_type, "Text",
            "Text (area=2000) should beat Group (area=4000) despite shorter RuntimeId");
    }

    /// Verify the OLD behavior (rid_len priority) was wrong.
    #[test]
    fn test_findall_rid_len_is_unreliable_depth_proxy() {
        let group = CandidateElement {
            index: 5, area: 4000, control_type: "Group".into(),
            name: String::new(), runtime_id: vec![42, 7, 3, 9, 15, 20],
        };
        let text = CandidateElement {
            index: 8, area: 2000, control_type: "Text".into(),
            name: "some text".into(), runtime_id: vec![42, 7, 3],
        };
        // NEW logic: area-based, Text wins
        assert!(candidate_dominates_findall(&text, group.area, group.index, &group.control_type, &group.name));
        assert!(!candidate_dominates_findall(&group, text.area, text.index, &text.control_type, &text.name));
        // OLD logic would have: group.runtime_id.len() > text.runtime_id.len() → wrong
        assert!(group.runtime_id.len() > text.runtime_id.len());
    }

    /// Same area → later index (deeper in traversal) wins.
    #[test]
    fn test_findall_same_area_later_index_wins() {
        let candidates = vec![
            CandidateElement { index: 3, area: 5000, control_type: "Group".into(), name: "outer".into(), runtime_id: vec![1, 2] },
            CandidateElement { index: 7, area: 5000, control_type: "Group".into(), name: "inner".into(), runtime_id: vec![3, 4] },
        ];
        let best = select_best_findall(&candidates).unwrap();
        assert_eq!(best.name, "inner");
        assert_eq!(best.index, 7);
    }

    /// Taskbar: Image (smaller) inside Button (larger).
    #[test]
    fn test_findall_taskbar_image_inside_button() {
        let candidates = vec![
            CandidateElement { index: 0, area: 2304, control_type: "Button".into(), name: "app".into(), runtime_id: vec![1, 2] },
            CandidateElement { index: 2, area: 576, control_type: "Image".into(), name: String::new(), runtime_id: vec![1, 2, 3] },
        ];
        let best = select_best_findall(&candidates).unwrap();
        assert_eq!(best.control_type, "Image");
    }

    /// Single candidate → selected.
    #[test]
    fn test_findall_single_candidate() {
        let candidates = vec![
            CandidateElement { index: 0, area: 100, control_type: "Text".into(), name: "hello".into(), runtime_id: vec![1] },
        ];
        let best = select_best_findall(&candidates).unwrap();
        assert_eq!(best.control_type, "Text");
    }

    /// Empty candidates → None.
    #[test]
    fn test_findall_no_candidates() {
        let candidates: Vec<CandidateElement> = vec![];
        assert!(select_best_findall(&candidates).is_none());
    }

    /// Deeply nested Chrome: Text leaf wins over all intermediate Groups.
    #[test]
    fn test_findall_chrome_deep_nesting_text_wins() {
        let candidates = vec![
            CandidateElement { index: 0, area: 480000, control_type: "Pane".into(), name: String::new(), runtime_id: vec![1] },
            CandidateElement { index: 1, area: 452400, control_type: "Document".into(), name: String::new(), runtime_id: vec![1,2,3] },
            CandidateElement { index: 2, area: 425600, control_type: "Group".into(), name: String::new(), runtime_id: vec![1,2,3,4,5] },
            CandidateElement { index: 3, area: 399600, control_type: "Group".into(), name: String::new(), runtime_id: vec![1,2,3,4,5,6,7] },
            CandidateElement { index: 4, area: 374400, control_type: "Group".into(), name: String::new(), runtime_id: vec![1,2,3] },
            CandidateElement { index: 5, area: 350000, control_type: "Group".into(), name: String::new(), runtime_id: vec![1,2,3,4,5,6] },
            CandidateElement { index: 6, area: 12000,  control_type: "Group".into(), name: String::new(), runtime_id: vec![1,2,3,4,5,6,7,8,9] },
            CandidateElement { index: 7, area: 6000,   control_type: "Text".into(), name: "text leaf".into(), runtime_id: vec![1,2] },
        ];
        let best = select_best_findall(&candidates).unwrap();
        assert_eq!(best.control_type, "Text",
            "Text leaf (area=6000) must win over Group (area=12000) despite shorter RuntimeId");
    }

    // ─── Leaf preference tests (based on real log scenarios) ─────────────

    /// LOG CASE 1: (527, 208) WeChat Chrome article title
    /// target='Group' name='' | ElementFromPoint type='Text' name='最高年费 5088！...'
    /// A tiny Group (empty name) was selected over a much larger Text (with name)
    /// because Group had a smaller area. Leaf preference fixes this.
    #[test]
    fn test_leaf_preference_text_beats_group_with_smaller_area() {
        let candidates = vec![
            // Tiny Group overlay/wrapper at the cursor point — empty name, small area
            CandidateElement {
                index: 5,
                area: 400,       // e.g. 20x20 tiny overlay
                control_type: "Group".into(),
                name: String::new(),
                runtime_id: vec![42, 7, 3, 9],
            },
            // The actual Text element the user wants — meaningful name, larger area
            CandidateElement {
                index: 8,
                area: 30000,     // e.g. 300x100 article title
                control_type: "Text".into(),
                name: "最高年费 5088！豆包收费让网友炸锅了，人民日报发声定调".into(),
                runtime_id: vec![42, 7, 3],
            },
        ];
        let best = select_best_findall(&candidates).unwrap();
        assert_eq!(best.control_type, "Text",
            "Text with meaningful name must beat tiny Group with empty name (leaf preference)");
        assert!(!best.name.is_empty());
    }

    /// LOG CASE 1 (variant): Group appears first in traversal but Text has name.
    /// Even if Group is evaluated first and becomes "best", Text must replace it.
    #[test]
    fn test_leaf_preference_group_first_then_text_replaces() {
        let candidates = vec![
            // Group evaluated first → becomes best (area=400, initially i64::MAX)
            CandidateElement {
                index: 3,
                area: 400,
                control_type: "Group".into(),
                name: String::new(),
                runtime_id: vec![1, 2, 3],
            },
            // Text evaluated later → must replace Group via leaf preference
            CandidateElement {
                index: 7,
                area: 30000,
                control_type: "Text".into(),
                name: "article title".into(),
                runtime_id: vec![4, 5],
            },
        ];
        let best = select_best_findall(&candidates).unwrap();
        assert_eq!(best.control_type, "Text");
    }

    /// LOG CASE 3: (551, 260) Both Group with empty name
    /// target='Group' name='' | ElementFromPoint type='Group' name=''
    /// When there's no leaf element, the smallest-area Group should still win.
    #[test]
    fn test_leaf_preference_no_leaf_available_group_wins() {
        let candidates = vec![
            CandidateElement {
                index: 0,
                area: 400000,
                control_type: "Pane".into(),
                name: String::new(),
                runtime_id: vec![1],
            },
            CandidateElement {
                index: 3,
                area: 12000,
                control_type: "Group".into(),
                name: String::new(),
                runtime_id: vec![1, 2, 3, 4],
            },
            CandidateElement {
                index: 6,
                area: 2000,
                control_type: "Group".into(),
                name: String::new(),
                runtime_id: vec![5, 6],
            },
        ];
        let best = select_best_findall(&candidates).unwrap();
        assert_eq!(best.control_type, "Group",
            "When no leaf available, smallest-area Group wins");
        assert_eq!(best.area, 2000);
    }

    /// LOG CASE 4: (482, 273) Text with name vs Group with empty name
    /// target='Text' name='程序员的那些事' | ElementFromPoint type='Group' name=''
    /// Text already has smaller area + leaf preference. Both mechanisms agree.
    #[test]
    fn test_leaf_preference_text_smaller_area_also_wins() {
        let candidates = vec![
            CandidateElement {
                index: 2,
                area: 20000,
                control_type: "Group".into(),
                name: String::new(),
                runtime_id: vec![1, 2, 3],
            },
            CandidateElement {
                index: 5,
                area: 5000,
                control_type: "Text".into(),
                name: "程序员的那些事".into(),
                runtime_id: vec![4, 5],
            },
        ];
        let best = select_best_findall(&candidates).unwrap();
        assert_eq!(best.control_type, "Text");
        assert_eq!(best.name, "程序员的那些事");
    }

    /// Leaf preference should NOT override when the container has a meaningful name.
    /// A Group with name (e.g., "Card") is a valid target.
    #[test]
    fn test_leaf_preference_not_applied_when_container_has_name() {
        let candidates = vec![
            CandidateElement {
                index: 2,
                area: 400,
                control_type: "Group".into(),
                name: "Card".into(),      // Container WITH name
                runtime_id: vec![1, 2, 3],
            },
            CandidateElement {
                index: 5,
                area: 30000,
                control_type: "Text".into(),
                name: "content".into(),
                runtime_id: vec![4, 5],
            },
        ];
        let best = select_best_findall(&candidates).unwrap();
        // Group with name and smaller area wins via normal area logic
        assert_eq!(best.control_type, "Group");
        assert_eq!(best.area, 400);
    }

    /// Leaf preference should NOT override when both are leaf types.
    /// Between two Text elements, smallest area should still win.
    #[test]
    fn test_leaf_preference_not_applied_between_two_leaves() {
        let candidates = vec![
            CandidateElement {
                index: 0,
                area: 5000,
                control_type: "Text".into(),
                name: "title".into(),
                runtime_id: vec![1],
            },
            CandidateElement {
                index: 3,
                area: 2000,
                control_type: "Text".into(),
                name: String::new(),     // Text with empty name — smaller area wins
                runtime_id: vec![2],
            },
        ];
        let best = select_best_findall(&candidates).unwrap();
        assert_eq!(best.area, 2000);
        // Text with empty name wins because both are leaves → area comparison
    }

    /// Leaf preference should NOT override when both are containers.
    #[test]
    fn test_leaf_preference_not_applied_between_two_containers() {
        let candidates = vec![
            CandidateElement {
                index: 0,
                area: 5000,
                control_type: "Group".into(),
                name: String::new(),
                runtime_id: vec![1],
            },
            CandidateElement {
                index: 3,
                area: 2000,
                control_type: "Pane".into(),
                name: String::new(),
                runtime_id: vec![2],
            },
        ];
        let best = select_best_findall(&candidates).unwrap();
        assert_eq!(best.control_type, "Pane"); // smaller area wins
        assert_eq!(best.area, 2000);
    }

    /// Leaf with empty name vs container with empty name → area wins.
    #[test]
    fn test_leaf_empty_name_vs_container_empty_name_area_wins() {
        let candidates = vec![
            CandidateElement {
                index: 0,
                area: 5000,
                control_type: "Group".into(),
                name: String::new(),
                runtime_id: vec![1],
            },
            CandidateElement {
                index: 3,
                area: 2000,
                control_type: "Text".into(),
                name: String::new(),    // Text with no name — no leaf preference
                runtime_id: vec![2],
            },
        ];
        let best = select_best_findall(&candidates).unwrap();
        // Text with empty name: leaf preference not triggered (c_has_name = false)
        // Falls back to area → Text (2000) wins
        assert_eq!(best.control_type, "Text");
        assert_eq!(best.area, 2000);
    }

    /// Full WeChat Chrome scenario: multiple Groups + one Text.
    #[test]
    fn test_wechat_chrome_full_scenario() {
        let candidates = vec![
            CandidateElement { index: 0, area: 480000, control_type: "Pane".into(), name: String::new(), runtime_id: vec![1] },
            CandidateElement { index: 1, area: 390000, control_type: "Group".into(), name: String::new(), runtime_id: vec![1,2] },
            CandidateElement { index: 2, area: 195000, control_type: "Group".into(), name: String::new(), runtime_id: vec![1,2,3] },
            // Tiny Group overlay at cursor — the bug trigger
            CandidateElement { index: 3, area: 400,    control_type: "Group".into(), name: String::new(), runtime_id: vec![1,2,3,4,5,6] },
            // The actual article title
            CandidateElement { index: 4, area: 30000,  control_type: "Text".into(), name: "article title".into(), runtime_id: vec![1,2,3,4] },
        ];
        let best = select_best_findall(&candidates).unwrap();
        assert_eq!(best.control_type, "Text",
            "Text with name must beat tiny Group overlay (leaf preference)");
        assert_eq!(best.name, "article title");
    }

    /// Leaf preference does not override when leaf has much larger area AND
    /// the container has a meaningful name (both have useful info).
    #[test]
    fn test_leaf_preference_container_named_vs_leaf_named() {
        let candidates = vec![
            CandidateElement {
                index: 0,
                area: 400,
                control_type: "Group".into(),
                name: "sidebar section".into(),  // Container WITH name
                runtime_id: vec![1, 2],
            },
            CandidateElement {
                index: 3,
                area: 30000,
                control_type: "Text".into(),
                name: "content text".into(),     // Leaf WITH name
                runtime_id: vec![3, 4],
            },
        ];
        let best = select_best_findall(&candidates).unwrap();
        // Both have names → leaf preference NOT triggered → area comparison
        assert_eq!(best.control_type, "Group",
            "When both have names, smallest area wins");
    }

    // ─── candidate_dominates_drilldown tests ─────────────────────────────

    #[test]
    fn test_drilldown_smaller_area_wins() {
        assert!(candidate_dominates_drilldown(100, 2, 200, 3));
    }

    #[test]
    fn test_drilldown_same_area_longer_rid_wins() {
        assert!(candidate_dominates_drilldown(100, 4, 100, 2));
    }

    #[test]
    fn test_drilldown_same_area_same_rid_no_dominate() {
        assert!(!candidate_dominates_drilldown(100, 3, 100, 3));
    }

    #[test]
    fn test_drilldown_larger_area_no_dominate() {
        assert!(!candidate_dominates_drilldown(200, 5, 100, 2));
    }

    #[test]
    fn test_drilldown_same_area_shorter_rid_no_dominate() {
        assert!(!candidate_dominates_drilldown(100, 2, 100, 5));
    }

    // ─── Integration-style tests ─────────────────────────────────────────

    /// WeChat scenario: Group (300x40) vs Text (300x20) at same point.
    #[test]
    fn test_wechat_scenario_group_vs_text() {
        let group_rect = SimpleRect { left: 400, top: 250, right: 700, bottom: 290 };
        let text_rect = SimpleRect { left: 400, top: 260, right: 700, bottom: 280 };
        assert!(point_in_rect(549, 269, &group_rect));
        assert!(point_in_rect(549, 269, &text_rect));

        let group_area = (group_rect.right - group_rect.left) as i64
            * (group_rect.bottom - group_rect.top) as i64;
        let text_area = (text_rect.right - text_rect.left) as i64
            * (text_rect.bottom - text_rect.top) as i64;

        let candidates = vec![
            CandidateElement { index: 5, area: group_area, control_type: "Group".into(), name: String::new(), runtime_id: vec![42, 7, 3, 9, 15, 20] },
            CandidateElement { index: 8, area: text_area, control_type: "Text".into(), name: "text content".into(), runtime_id: vec![42, 7, 3] },
        ];
        let best = select_best_findall(&candidates).unwrap();
        assert_eq!(best.control_type, "Text",
            "In WeChat Chrome, Text (area={}) must win over Group (area={})", text_area, group_area);
    }

    /// Group only (no Text child) → Group is selected.
    #[test]
    fn test_findall_group_only_selected_when_no_text() {
        let candidates = vec![
            CandidateElement { index: 0, area: 4000, control_type: "Group".into(), name: String::new(), runtime_id: vec![1, 2] },
        ];
        let best = select_best_findall(&candidates).unwrap();
        assert_eq!(best.control_type, "Group");
    }

    /// Same area, same index → leaf preference makes Text dominate Group with no name.
    #[test]
    fn test_findall_identical_area_same_index_leaf_wins() {
        let c1_area = 1000i64;
        let c1_index = 3i32;
        let c1_ct = "Group".to_string();
        let c1_name = String::new();
        let c2 = CandidateElement { index: 3, area: 1000, control_type: "Text".into(), name: "second".into(), runtime_id: vec![2] };
        // Leaf preference: Text(name="second") dominates Group(name="") even with same area+index
        assert!(candidate_dominates_findall(&c2, c1_area, c1_index, &c1_ct, &c1_name));
    }

    /// Same area, same index, both same type with name → no flip.
    #[test]
    fn test_findall_identical_area_same_index_same_type_no_flip() {
        let c1_area = 1000i64;
        let c1_index = 3i32;
        let c1_ct = "Text".to_string();
        let c1_name = "first".to_string();
        let c2 = CandidateElement { index: 3, area: 1000, control_type: "Text".into(), name: "second".into(), runtime_id: vec![2] };
        // Both are leaves with names → no leaf preference → same area+index → no flip
        assert!(!candidate_dominates_findall(&c2, c1_area, c1_index, &c1_ct, &c1_name));
    }
}

