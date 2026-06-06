use super::*;
use uiauto_xpath::control_type_id_to_name;

pub fn exists_window_by_selector(window_selector: &str) -> bool {
    debug!("Checking window existence: {}", window_selector);
    
    let auto = match get_automation() {
        Ok(a) => a,
        Err(_) => return false,
    };

    let windows = find_window_by_selector(&auto, window_selector);
    !windows.is_empty()
}

pub fn activate_window_by_selector(window_selector: &str) -> bool {
    debug!("Activating window: {}", window_selector);
    
    let auto = match get_automation() {
        Ok(a) => a,
        Err(_) => return false,
    };

    // Find the window element(s)
    let windows = find_window_by_selector(&auto, window_selector);
    let window_element = match windows.first() {
        Some(w) => w,
        None => {
            error!("Window not found for activation: {}", window_selector);
            return false;
        }
    };

    // Use SetFocus to activate the window (brings to foreground)
    window_element.set_focus().ok().is_some()
}

pub fn activate_and_focus_element(window_selector: &str, xpath: &str) -> bool {
    debug!("Activating window and focusing element: {} / {}", window_selector, xpath);
    
    let auto = match get_automation() {
        Ok(a) => a,
        Err(_) => return false,
    };

    // Find all matching windows and try each
    let windows = find_window_by_selector(&auto, window_selector);
    if windows.is_empty() {
        return false;
    }

    for window_element in &windows {
        // Activate window via SetFocus
        if window_element.set_focus().is_err() {
            continue;
        }
        
        // Small delay for window activation
        std::thread::sleep(std::time::Duration::from_millis(100));
        
        // Find target element using find_by_xpath_detailed
        if let Ok((elements, _)) = execute_xpath_steps_filtered(&auto, window_element, xpath, &FindAllFilter::default(), Some(5000)) {
            if !elements.is_empty() {
                if elements[0].set_focus().ok().is_some() {
                    return true;
                }
            }
        }
    }
    
    false
}

pub fn get_window_rect_by_selector(window_selector: &str) -> Option<crate::core::model::ElementRect> {
    let auto = match get_automation() {
        Ok(a) => a,
        Err(_) => return None,
    };

    let windows = find_window_by_selector(&auto, window_selector);
    let window_element = windows.first()?;

    let rect = window_element.get_bounding_rectangle()
        .map(|r| crate::core::model::ElementRect {
            x: r.get_left(),
            y: r.get_top(),
            width: r.get_right() - r.get_left(),
            height: r.get_bottom() - r.get_top(),
        })
        .ok();

    rect
}

pub(super) fn find_window_by_selector(
    auto: &UIAutomation,
    window_selector: &str,
) -> Vec<UIElement> {
    use std::time::Instant;
    
    let start = Instant::now();
    let (expected_name, expected_class, expected_process) = parse_window_selector(window_selector);
    
    // Fast path: EnumWindows + ElementFromHandle
    let fast_results = find_window_by_enum_windows(
        auto, &expected_name, &expected_class, &expected_process,
    );
    if !fast_results.is_empty() {
        log::info!("[Window Search] EnumWindows fast path found {} window(s) in {}ms", 
            fast_results.len(), start.elapsed().as_millis());
        return fast_results;
    }
    
    log::info!("[Window Search] EnumWindows found 0, falling back to UIA search...");
    
    // Fallback: UIA condition-based search
    let fallback_results = find_window_by_uia_condition(
        auto, &expected_name, &expected_class, &expected_process,
    );
    log::info!("[Window Search] UIA fallback found {} window(s) in {}ms total", 
        fallback_results.len(), start.elapsed().as_millis());
    
    fallback_results
}

fn find_window_by_enum_windows(
    auto: &UIAutomation,
    expected_name: &Option<String>,
    expected_class: &Option<String>,
    expected_process: &Option<String>,
) -> Vec<UIElement> {
    // Collect matching HWNDs via EnumWindows callback
    let matched_hwnds: Vec<HWND> = enumerate_top_level_windows();

    if matched_hwnds.is_empty() {
        return vec![];
    }

    // Create cache request to batch-prefetch Name + ClassName (2-3x faster)
    let window_cache = create_window_cache_request(auto);

    let mut results = Vec::new();

    for hwnd in matched_hwnds {
        // Skip invisible windows early
        if !unsafe { IsWindowVisible(hwnd) }.as_bool() {
            continue;
        }

        // Check PID if ProcessName filter is specified
        if let Some(ref expected_proc) = expected_process {
            let mut pid: u32 = 0;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
            if pid == 0 {
                continue;
            }
            let proc_name = get_process_name_by_id(pid);
            if &proc_name != expected_proc {
                continue;
            }
        }

        // Convert HWND to UIA element (with cache prefetch if available)
        let elem = match &window_cache {
            Some(cr) => match auto.element_from_handle_build_cache(hwnd.into(), cr) {
                Ok(e) => e,
                Err(_) => continue,
            },
            None => match auto.element_from_handle(hwnd.into()) {
                Ok(e) => e,
                Err(_) => continue,
            },
        };

        // Verify Name condition
        if let Some(ref expected) = expected_name {
            let actual_name = if window_cache.is_some() {
                elem.get_cached_name().unwrap_or_default()
            } else {
                elem.get_name().unwrap_or_default()
            };
            if &actual_name != expected {
                continue;
            }
        }

        // Verify ClassName condition
        if let Some(ref expected) = expected_class {
            let actual_class = if window_cache.is_some() {
                elem.get_cached_classname().unwrap_or_default()
            } else {
                elem.get_classname().unwrap_or_default()
            };
            if &actual_class != expected {
                continue;
            }
        }

        results.push(elem);
    }

    results
}

pub(super) fn enumerate_top_level_windows() -> Vec<HWND> {
    let hwnds: std::cell::RefCell<Vec<HWND>> = std::cell::RefCell::new(Vec::new());
    
    // EnumWindows callback: collect HWNDs of visible windows
    unsafe extern "system" fn enum_callback(hwnd: HWND, lparam: LPARAM) -> WinBool {
        let hwnds = &*(lparam.0 as *const std::cell::RefCell<Vec<HWND>>);
        // Only collect visible windows
        if IsWindowVisible(hwnd).as_bool() {
            hwnds.borrow_mut().push(hwnd);
        }
        WinBool(1) // Continue enumeration
    }
    
    let hwnds_ptr = &hwnds as *const _ as isize;
    unsafe {
        let _ = EnumWindows(
            Some(enum_callback),
            LPARAM(hwnds_ptr),
        );
    }
    
    hwnds.into_inner()
}

fn find_window_by_uia_condition(
    auto: &UIAutomation,
    expected_name: &Option<String>,
    expected_class: &Option<String>,
    expected_process: &Option<String>,
) -> Vec<UIElement> {
    let desktop = match auto.get_root_element() {
        Ok(d) => d,
        Err(_) => return vec![],
    };
    
    // Build UIA condition for engine-level filtering
    let condition = build_window_find_condition(auto, expected_name, expected_class);
    
    let windows = match desktop.find_all(TreeScope::Children, &condition) {
        Ok(w) => w,
        Err(_) => return vec![],
    };
    
    let mut matched_windows = Vec::new();
    
    for win in &windows {
        let pid = win.get_process_id().unwrap_or(0);
        
        if let Some(ref expected_proc) = expected_process {
            let process_name = get_process_name_by_id(pid);
            if &process_name != expected_proc {
                continue;
            }
        }
        
        matched_windows.push(win.clone());
    }
    
    matched_windows
}

fn build_window_find_condition(
    auto: &UIAutomation,
    expected_name: &Option<String>,
    expected_class: &Option<String>,
) -> UICondition {
    let mut conditions: Vec<UICondition> = Vec::new();
    
    // Add Name condition if specified
    if let Some(ref name) = expected_name {
        if let Ok(cond) = auto.create_property_condition(
            UIProperty::Name,
            Variant::from(name.as_str()),
            Some(PropertyConditionFlags::IgnoreCase),
        ) {
            conditions.push(cond);
            log::debug!("[Window Search] Added Name condition: '{}'", name);
        }
    }
    
    // Add ClassName condition if specified
    if let Some(ref class) = expected_class {
        if let Ok(cond) = auto.create_property_condition(
            UIProperty::ClassName,
            Variant::from(class.as_str()),
            Some(PropertyConditionFlags::IgnoreCase),
        ) {
            conditions.push(cond);
            log::debug!("[Window Search] Added ClassName condition: '{}'", class);
        }
    }
    
    // Combine conditions
    match conditions.len() {
        0 => {
            // No specific conditions, use TrueCondition
            auto.create_true_condition().unwrap_or_else(|e| {
                log::error!("CreateTrueCondition failed: {}", e);
                panic!("CreateTrueCondition failed — no fallback available");
            })
        }
        1 => conditions.remove(0),
        _ => {
            // Combine conditions with AND (chaining create_and_condition)
            let mut combined: Option<UICondition> = Some(conditions.remove(0));
            for cond in conditions {
                let current = match combined.take() {
                    Some(c) => c,
                    None => break,
                };
                match auto.create_and_condition(current, cond) {
                    Ok(c) => combined = Some(c),
                    Err(e) => {
                        log::error!("[Window Search] CreateAndCondition failed: {}", e);
                        break;
                    }
                }
            }
            combined.unwrap_or_else(|| auto.create_true_condition().expect("CreateTrueCondition failed"))
        }
    }
}

fn parse_window_selector(selector: &str) -> (Option<String>, Option<String>, Option<String>) {
    let mut name = None;
    let mut class = None;
    let mut process_name = None;
    
    // Extract content between [ and ]
    if let Some(start) = selector.find('[') {
        if let Some(end) = selector.rfind(']') {
            let predicates = &selector[start + 1..end];
            
            // Parse @Name='value'
            if let Some(pos) = predicates.find("@Name='") {
                let start_pos = pos + 7;
                if let Some(end_pos) = predicates[start_pos..].find('\'') {
                    name = Some(predicates[start_pos..start_pos + end_pos].to_string());
                }
            }
            
            // Parse @ClassName='value'
            if let Some(pos) = predicates.find("@ClassName='") {
                let start_pos = pos + 12;
                if let Some(end_pos) = predicates[start_pos..].find('\'') {
                    class = Some(predicates[start_pos..start_pos + end_pos].to_string());
                }
            }
            
            // Parse @ProcessName='value'
            if let Some(pos) = predicates.find("@ProcessName='") {
                let start_pos = pos + 14;
                if let Some(end_pos) = predicates[start_pos..].find('\'') {
                    process_name = Some(predicates[start_pos..start_pos + end_pos].to_string());
                }
            }
        }
    }
    
    (name, class, process_name)
}

pub(super) fn find_content_root(auto: &UIAutomation, window: &UIElement) -> Option<UIElement> {
    let walker = auto.get_raw_view_walker().ok()
        .or_else(|| auto.get_control_view_walker().ok())?;
    let window_fwid = window.get_framework_id().unwrap_or_default();
    
    log::info!("[Content Root] Window FrameworkId='{}', scanning children...", window_fwid);
    
    // Create cache request for window child scanning
    let window_cache = create_window_cache_request(auto);
    
    // Track same-framework container candidates for fallback
    let mut same_framework_container: Option<UIElement> = None;
    
    let mut child = match &window_cache {
        Some(cr) => walker.get_first_child_build_cache(window, cr).ok(),
        None => walker.get_first_child(window).ok(),
    };
    let mut idx = 0;
    while let Some(c) = child {
        let ct = if window_cache.is_some() {
            c.get_cached_control_type().map(|ct| control_type_id_to_name(ct as i32).to_string()).unwrap_or_default()
        } else {
            c.get_control_type_raw().map(control_type_name).unwrap_or_default()
        };
        let fwid = if window_cache.is_some() {
            c.get_cached_framework_id().unwrap_or_default()
        } else {
            c.get_framework_id().unwrap_or_default()
        };
        let class = if window_cache.is_some() {
            c.get_cached_classname().unwrap_or_default()
        } else {
            c.get_classname().unwrap_or_default()
        };
        
        // Skip non-container types (TitleBar, etc.)
        if ct != "Pane" && ct != "Window" && ct != "Group" {
            child = match &window_cache {
                Some(cr) => walker.get_next_sibling_build_cache(&c, cr).ok(),
                None => walker.get_next_sibling(&c).ok(),
            };
            idx += 1;
            continue;
        }
        
        // Skip known rendering layers
        if is_rendering_layer(&class) {
            log::info!("[Content Root] Skipping rendering layer at child[{}]: class='{}' fw='{}'", 
                idx, class, fwid);
            child = match &window_cache {
                Some(cr) => walker.get_next_sibling_build_cache(&c, cr).ok(),
                None => walker.get_next_sibling(&c).ok(),
            };
            idx += 1;
            continue;
        }
        
        // Strategy 1: Match well-known WebView container class names
        if is_webview_class(&class) {
            log::info!("[Content Root] Found WebView container at child[{}]: class='{}' (FrameworkId='{}')", 
                idx, class, fwid);
            return Some(c);
        }
        
        // Strategy 2: Check for framework transition (including deep search)
        if !fwid.is_empty() && fwid != window_fwid {
            log::info!("[Content Root] Found framework transition at child[{}]: '{}' → '{}' (class='{}')", 
                idx, window_fwid, fwid, class);
            return Some(c);
        }
        
        // Track first same-framework container as fallback candidate
        if same_framework_container.is_none() && fwid == window_fwid && !fwid.is_empty() {
            same_framework_container = Some(c.clone());
        }
        
        // Deep search: check if any descendant (up to 6 levels) has different FrameworkId
        if has_framework_transition(&walker, &c, &window_fwid, 6) {
            log::info!("[Content Root] Found deep framework transition under child[{}]: class='{}'", 
                idx, class);
            return Some(c);
        }
        
        child = match &window_cache {
            Some(cr) => walker.get_next_sibling_build_cache(&c, cr).ok(),
            None => walker.get_next_sibling(&c).ok(),
        };
        idx += 1;
        if idx > 10 { break; }
    }
    
    // Fallback: if no framework transition found, return the first container child
    // with the same FrameworkId as the window.
    if let Some(ref elem) = same_framework_container {
        let class = elem.get_classname().unwrap_or_default();
        log::info!("[Content Root] No framework transition found, using same-framework container: class='{}'", class);
        return Some(elem.clone());
    }
    
    log::info!("[Content Root] No content root found");
    None
}

fn has_framework_transition(
    walker: &UITreeWalker,
    elem: &UIElement,
    parent_fwid: &str,
    max_depth: usize,
) -> bool {
    use std::collections::VecDeque;
    
    // Use BFS (breadth-first search) instead of DFS to avoid stack overflow
    let mut queue: VecDeque<(UIElement, usize)> = VecDeque::new();
    
    // Add direct children to queue
    let mut child = walker.get_first_child(elem).ok();
    while let Some(c) = child {
        let next = walker.get_next_sibling(&c).ok();
        queue.push_back((c, 1));
        child = next;
    }
    
    let mut visited_count = 0;
    const MAX_NODES: usize = 150;
    
    while let Some((node, depth)) = queue.pop_front() {
        visited_count += 1;
        if visited_count > MAX_NODES {
            log::debug!("[Content Root] BFS limit reached ({} nodes), stopping", MAX_NODES);
            return false;
        }
        
        if depth > max_depth {
            continue;
        }
        
        let sub_fwid = node.get_framework_id().unwrap_or_default();
        let sub_ct = node.get_control_type_raw().map(control_type_name).unwrap_or_default();
        
        // Early pruning: skip non-container types at deeper levels
        if depth > 2 && sub_ct != "Pane" && sub_ct != "Window" && sub_ct != "Group" && sub_ct != "Document" {
            continue;
        }
        
        // Found a different FrameworkId → transition detected
        if !sub_fwid.is_empty() && sub_fwid != parent_fwid {
            log::info!("[Content Root] Found framework transition at depth {}: '{}'", depth, sub_fwid);
            return true;
        }
        
        // Add children to queue for next level (with dynamic breadth limiting)
        if depth < max_depth {
            let mut sub_child = walker.get_first_child(&node).ok();
            let mut sub_count = 0;
            let max_children = if depth <= 2 { 15 } else { 8 };
            while let Some(sc) = sub_child {
                let next_sc = walker.get_next_sibling(&sc).ok();
                queue.push_back((sc, depth + 1));
                sub_child = next_sc;
                sub_count += 1;
                if sub_count > max_children { break; }
            }
        }
    }
    
    false
}

pub(super) fn find_sibling_windows_same_process(
    auto: &UIAutomation,
    window: &UIElement,
) -> anyhow::Result<Vec<UIElement>> {
    // Get the process ID of the reference window
    let process_id = window.get_process_id()? as i32;
    log::debug!("[Sibling Search] Looking for windows with process ID: {}", process_id);
    
    // Create condition to match the same process ID
    let condition = auto.create_property_condition(
        UIProperty::ProcessId,
        Variant::from(process_id),
        None,
    )?;
    
    // Find all elements with this process ID from Desktop root
    // TreeScope::Children only searches immediate children (top-level windows)
    let desktop = auto.get_root_element()?;
    let elements = desktop.find_all(TreeScope::Children, &condition)?;
    
    log::debug!("[Sibling Search] Found {} windows with same process ID", elements.len());
    
    // Get the HWND of the original window for comparison
    let original_handle = window.get_native_window_handle().ok();
    
    let mut siblings = Vec::new();
    for elem in &elements {
        // Skip the original window itself by comparing HWND
        if let Some(ref orig_h) = original_handle {
            if let Ok(elem_h) = elem.get_native_window_handle() {
                // Compare handles: if they are the same, skip
                let orig_raw: windows::Win32::Foundation::HANDLE = (*orig_h).into();
                let elem_raw: windows::Win32::Foundation::HANDLE = elem_h.into();
                if orig_raw.0 == elem_raw.0 {
                    continue;
                }
            }
        }
        
        siblings.push(elem.clone());
    }
    
    log::debug!("[Sibling Search] Returning {} sibling windows (excluding original)", siblings.len());
    Ok(siblings)
}

pub(super) fn find_child_process_windows(
    auto: &UIAutomation,
    parent_window: &UIElement,
) -> anyhow::Result<Vec<UIElement>> {
    use std::collections::HashSet;
    
    // Get parent process info
    let parent_pid = parent_window.get_process_id()?;
    let parent_process_name = get_process_name_by_id(parent_pid);
    log::info!("[Child Process Search] Parent PID={}, ProcessName='{}'", parent_pid, parent_process_name);
    
    // Enumerate all windows and filter by process name
    let mut candidate_windows = Vec::new();
    let mut related_pids: HashSet<u32> = HashSet::new();
    
    // Use EnumWindows to efficiently get all top-level windows
    let windows_list = crate::core::enum_windows::enumerate_windows_fast();
    log::info!("[Child Process Search] Found {} total windows via EnumWindows", windows_list.len());
    
    for win_info in windows_list.iter() {
        // Skip the parent process itself
        if win_info.process_id == parent_pid {
            continue;
        }
        
        // Skip if we've already seen this PID
        if !related_pids.contains(&win_info.process_id) {
            // Check if this process name is related to the parent
            let proc_name_lower = win_info.process_name.to_lowercase();
            let parent_name_lower = parent_process_name.to_lowercase();
            
            let is_related = (!proc_name_lower.is_empty() && !parent_name_lower.is_empty()) && (
                proc_name_lower == parent_name_lower
                || proc_name_lower.starts_with(&parent_name_lower)
                || parent_name_lower.starts_with(&proc_name_lower)
                || (parent_name_lower.contains("weixin") && proc_name_lower.contains("wechat"))
                || (parent_name_lower.contains("wechat") && proc_name_lower.contains("weixin"))
            );
            
            if is_related {
                related_pids.insert(win_info.process_id);
                log::info!("[Child Process Search] Found related process: PID={}, Name='{}', Title='{}'", 
                    win_info.process_id, win_info.process_name, win_info.title);
            }
        }
    }
    
    // For matched PIDs, find UIA elements via desktop search
    if !related_pids.is_empty() {
        let desktop = auto.get_root_element()?;
        let true_cond = auto.create_true_condition()?;
        let all_windows = desktop.find_all(TreeScope::Children, &true_cond)?;
        
        for elem in &all_windows {
            if let Ok(elem_pid) = elem.get_process_id() {
                if related_pids.contains(&elem_pid) {
                    let class_name = elem.get_classname().unwrap_or_default();
                    if let Ok(handle) = elem.get_native_window_handle() {
                        let raw_handle: windows::Win32::Foundation::HANDLE = handle.into();
                        let hwnd = HWND(raw_handle.0);
                        let is_visible = unsafe { IsWindowVisible(hwnd) }.as_bool();
                        if is_visible {
                            log::info!("[Child Process Search] Adding UIA window: PID={}, Class='{}'", 
                                elem_pid, class_name);
                            candidate_windows.push(elem.clone());
                        }
                    }
                }
            }
        }
    }
    
    log::info!("[Child Process Search] Found {} candidate windows from related processes", candidate_windows.len());
    Ok(candidate_windows)
}

#[allow(dead_code)]
fn check_window_visibility(hwnd: HWND) -> anyhow::Result<bool> {
    Ok(unsafe { IsWindowVisible(hwnd).as_bool() })
}

pub(super) fn parse_positional_predicate(s: &str) -> Option<usize> {
    let s = s.trim();
    if s.starts_with('[') && s.ends_with(']') && s.len() >= 3 {
        let inner = &s[1..s.len()-1];
        inner.parse::<usize>().ok()
    } else {
        None
    }
}

pub(super) fn enum_child_hwnds(parent: HWND) -> Vec<HWND> {
    const MAX_CHILD_HWNDS: usize = 64;
    let hwnds: std::cell::RefCell<Vec<HWND>> = std::cell::RefCell::new(Vec::new());
    
    unsafe extern "system" fn enum_callback(child: HWND, lparam: LPARAM) -> WinBool {
        let hwnds = &*(lparam.0 as *const std::cell::RefCell<Vec<HWND>>);
        // Skip invisible windows and cap at MAX_CHILD_HWNDS
        if !IsWindowVisible(child).as_bool() {
            return WinBool(1); // Continue enumeration
        }
        let mut vec = hwnds.borrow_mut();
        if vec.len() >= MAX_CHILD_HWNDS {
            return WinBool(0); // Stop enumeration
        }
        vec.push(child);
        WinBool(1) // Continue enumeration
    }
    
    let hwnds_ptr = &hwnds as *const _ as isize;
    unsafe {
        let _ = EnumChildWindows(
            Some(parent),
            Some(enum_callback),
            LPARAM(hwnds_ptr),
        );
    }
    
    hwnds.into_inner()
}
