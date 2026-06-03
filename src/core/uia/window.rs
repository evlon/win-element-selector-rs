use super::*;

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
    unsafe {
        window_element.SetFocus().ok().is_some()
    }
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
        if unsafe { window_element.SetFocus() }.is_err() {
            continue;
        }
        
        // Small delay for window activation
        std::thread::sleep(std::time::Duration::from_millis(100));
        
        // Find target element using find_by_xpath_detailed
        if let Ok((elements, _)) = find_by_xpath_with_fallback(&auto, window_element, xpath) {
            if !elements.is_empty() {
                if unsafe { elements[0].SetFocus() }.ok().is_some() {
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

    let rect = unsafe {
        window_element.CurrentBoundingRectangle()
            .map(|r| crate::core::model::ElementRect {
                x: r.left,
                y: r.top,
                width: r.right - r.left,
                height: r.bottom - r.top,
            })
            .ok()
    };

    rect
}

pub(super) fn find_window_by_selector(
    auto: &IUIAutomation,
    window_selector: &str,
) -> Vec<IUIAutomationElement> {
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
    auto: &IUIAutomation,
    expected_name: &Option<String>,
    expected_class: &Option<String>,
    expected_process: &Option<String>,
) -> Vec<IUIAutomationElement> {
    // Collect matching HWNDs via EnumWindows callback
    let matched_hwnds: Vec<HWND> = enumerate_top_level_windows();
    
    if matched_hwnds.is_empty() {
        return vec![];
    }
    
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
        
        // Convert HWND to UIA element
        let elem = match unsafe { auto.ElementFromHandle(hwnd) } {
            Ok(e) => e,
            Err(_) => continue,
        };
        
        // Verify Name condition
        if let Some(ref expected) = expected_name {
            let actual_name = get_bstr(unsafe { elem.CurrentName() });
            if &actual_name != expected {
                continue;
            }
        }
        
        // Verify ClassName condition
        if let Some(ref expected) = expected_class {
            let actual_class = get_bstr(unsafe { elem.CurrentClassName() });
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
    unsafe extern "system" fn enum_callback(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let hwnds = &*(lparam.0 as *const std::cell::RefCell<Vec<HWND>>);
        // Only collect visible windows
        if IsWindowVisible(hwnd).as_bool() {
            hwnds.borrow_mut().push(hwnd);
        }
        windows::core::BOOL(1) // Continue enumeration
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
    auto: &IUIAutomation,
    expected_name: &Option<String>,
    expected_class: &Option<String>,
    expected_process: &Option<String>,
) -> Vec<IUIAutomationElement> {
    use windows::Win32::UI::Accessibility::*;
    
    let desktop = match unsafe { auto.GetRootElement() } {
        Ok(d) => d,
        Err(_) => return vec![],
    };
    
    // Build UIA condition for engine-level filtering
    let condition = build_window_find_condition(auto, expected_name, expected_class);
    
    let windows = match unsafe { desktop.FindAll(TreeScope_Children, &condition) } {
        Ok(w) => w,
        Err(_) => return vec![],
    };
    
    let count = match unsafe { windows.Length() } {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    
    let mut matched_windows = Vec::new();
    
    for i in 0..count {
        let win = match unsafe { windows.GetElement(i) } {
            Ok(w) => w,
            Err(_) => continue,
        };
        
        let pid = unsafe { win.CurrentProcessId().ok() }.unwrap_or(0) as u32;
        
        if let Some(ref expected_proc) = expected_process {
            let process_name = get_process_name_by_id(pid);
            if &process_name != expected_proc {
                continue;
            }
        }
        
        matched_windows.push(win);
    }
    
    matched_windows
}

fn build_window_find_condition(
    auto: &IUIAutomation,
    expected_name: &Option<String>,
    expected_class: &Option<String>,
) -> windows::Win32::UI::Accessibility::IUIAutomationCondition {
    use windows::Win32::UI::Accessibility::*;
    use windows::Win32::System::Variant::*;
    use windows::core::BSTR;
    
    // Helper: create a property condition for a BSTR value
    let try_bstr_condition = |prop_id: UIA_PROPERTY_ID, value: &str| -> Option<IUIAutomationCondition> {
        let mut variant = VARIANT::default();
        unsafe {
            // Use raw pointer writes to avoid ManuallyDrop DerefMut issues
            let var_ptr = &mut variant as *mut VARIANT;
            // VARIANT layout: vt (u16) at offset 0, then padding, then union at offset 8
            // Write VT_BSTR to the vt field
            let vt_ptr = var_ptr as *mut VARENUM;
            std::ptr::write(vt_ptr, VT_BSTR);
            // Write BSTR to the bstrVal field in the union
            // The union starts at offset 8 (after vt + 3 reserved u16s)
            let bstr_ptr = (var_ptr as *mut u8).add(8) as *mut core::mem::ManuallyDrop<BSTR>;
            std::ptr::write(bstr_ptr, core::mem::ManuallyDrop::new(BSTR::from(value)));
            auto.CreatePropertyCondition(prop_id, &variant).ok()
        }
    };
    
    let mut conditions: Vec<IUIAutomationCondition> = Vec::new();
    
    // Add Name condition if specified
    if let Some(ref name) = expected_name {
        if let Some(cond) = try_bstr_condition(UIA_NamePropertyId, name) {
            conditions.push(cond);
            log::debug!("[Window Search] Added Name condition: '{}'", name);
        }
    }
    
    // Add ClassName condition if specified
    if let Some(ref class) = expected_class {
        if let Some(cond) = try_bstr_condition(UIA_ClassNamePropertyId, class) {
            conditions.push(cond);
            log::debug!("[Window Search] Added ClassName condition: '{}'", class);
        }
    }
    
    // Combine conditions
    match conditions.len() {
        0 => {
            // No specific conditions, use TrueCondition
            unsafe { auto.CreateTrueCondition() }.unwrap_or_else(|e| {
                log::error!("CreateTrueCondition failed: {}", e);
                // 使用第一个可用的条件作为 fallback（不应该到这里，因为 conditions 为空）
                panic!("CreateTrueCondition failed — no fallback available");
            })
        }
        1 => conditions.remove(0),
        2 => {
            // Combine two conditions with AND
            let cond2 = conditions.remove(1);
            let cond1 = conditions.remove(0);
            unsafe {
                auto.CreateAndCondition(&cond1, &cond2)
                    .unwrap_or(cond1)
            }
        }
        _ => {
            // Combine multiple conditions with AND using CreateAndConditionFromNativeArray
            let opts: Vec<Option<IUIAutomationCondition>> = conditions.into_iter().map(Some).collect();
            let first = opts.first().and_then(|o| o.clone());
            unsafe {
                auto.CreateAndConditionFromNativeArray(&opts)
                    .unwrap_or_else(|_| first.expect("at least one condition required"))
            }
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

pub(super) fn find_content_root(auto: &IUIAutomation, window: &IUIAutomationElement) -> Option<IUIAutomationElement> {
    let walker = unsafe { auto.RawViewWalker().ok() }
        .or_else(|| unsafe { auto.ControlViewWalker().ok() })?;
    let window_fwid = get_bstr(unsafe { window.CurrentFrameworkId() });
    
    log::info!("[Content Root] Window FrameworkId='{}', scanning children...", window_fwid);
    
    // Track same-framework container candidates for fallback
    let mut same_framework_container: Option<IUIAutomationElement> = None;
    
    let mut child = unsafe { walker.GetFirstChildElement(window).ok() };
    let mut idx = 0;
    while let Some(c) = child {
        let ct = unsafe { c.CurrentControlType().map(control_type_name).unwrap_or_default() };
        let fwid = get_bstr(unsafe { c.CurrentFrameworkId() });
        let class = get_bstr(unsafe { c.CurrentClassName() });
        
        // Skip non-container types (TitleBar, etc.)
        if ct != "Pane" && ct != "Window" && ct != "Group" {
            child = unsafe { walker.GetNextSiblingElement(&c).ok() };
            idx += 1;
            continue;
        }
        
        // Skip known rendering layers (e.g., MMUIRenderSubWindowHW in WeChat).
        // These are hardware rendering surfaces with a different FrameworkId,
        // but they do NOT contain accessible content — the actual content is
        // in a sibling container with the same FrameworkId as the window.
        if is_rendering_layer(&class) {
            log::info!("[Content Root] Skipping rendering layer at child[{}]: class='{}' fw='{}'", 
                idx, class, fwid);
            child = unsafe { walker.GetNextSiblingElement(&c).ok() };
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
        // If this child or any descendant has a different FrameworkId, this is the content root
        if !fwid.is_empty() && fwid != window_fwid {
            log::info!("[Content Root] Found framework transition at child[{}]: '{}' → '{}' (class='{}')", 
                idx, window_fwid, fwid, class);
            return Some(c);
        }
        
        // Track first same-framework container as fallback candidate
        // (e.g., QWidget with same FrameworkId='Qt' as the window)
        if same_framework_container.is_none() && fwid == window_fwid && !fwid.is_empty() {
            same_framework_container = Some(c.clone());
        }
        
        // Deep search: check if any descendant (up to 6 levels) has different FrameworkId
        if has_framework_transition(&walker, &c, &window_fwid, 6) {
            log::info!("[Content Root] Found deep framework transition under child[{}]: class='{}'", 
                idx, class);
            return Some(c);
        }
        
        child = unsafe { walker.GetNextSiblingElement(&c).ok() };
        idx += 1;
        if idx > 10 { break; }
    }
    
    // Fallback: if no framework transition found (all rendering layers were skipped),
    // return the first container child with the same FrameworkId as the window.
    // This handles WeChat-like apps where the content is in a QWidget sibling
    // of the MMUIRenderSubWindowHW rendering layer.
    if let Some(ref elem) = same_framework_container {
        let class = get_bstr(unsafe { elem.CurrentClassName() });
        log::info!("[Content Root] No framework transition found, using same-framework container: class='{}'", class);
        return Some(elem.clone());
    }
    
    log::info!("[Content Root] No content root found");
    None
}

fn has_framework_transition(
    walker: &IUIAutomationTreeWalker,
    elem: &IUIAutomationElement,
    parent_fwid: &str,
    max_depth: usize,
) -> bool {
    use std::collections::VecDeque;
    
    // Use BFS (breadth-first search) instead of DFS to avoid stack overflow
    let mut queue: VecDeque<(IUIAutomationElement, usize)> = VecDeque::new();
    
    // Add direct children to queue
    let mut child = unsafe { walker.GetFirstChildElement(elem).ok() };
    while let Some(c) = child {
        let next = unsafe { walker.GetNextSiblingElement(&c).ok() };
        queue.push_back((c, 1));
        child = next;
    }
    
    let mut visited_count = 0;
    const MAX_NODES: usize = 150; // Increased from 100 for better coverage
    
    while let Some((node, depth)) = queue.pop_front() {
        visited_count += 1;
        if visited_count > MAX_NODES {
            log::debug!("[Content Root] BFS limit reached ({} nodes), stopping", MAX_NODES);
            return false;
        }
        
        if depth > max_depth {
            continue;
        }
        
        let sub_fwid = get_bstr(unsafe { node.CurrentFrameworkId() });
        let sub_ct = unsafe { node.CurrentControlType().map(control_type_name).unwrap_or_default() };
        
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
            let mut sub_child = unsafe { walker.GetFirstChildElement(&node).ok() };
            let mut sub_count = 0;
            // Dynamic breadth: more children at shallow levels, fewer at deep levels
            let max_children = if depth <= 2 { 15 } else { 8 };
            while let Some(sc) = sub_child {
                let next_sc = unsafe { walker.GetNextSiblingElement(&sc).ok() };
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
    auto: &IUIAutomation,
    window: &IUIAutomationElement,
) -> anyhow::Result<Vec<IUIAutomationElement>> {
    use windows::Win32::UI::Accessibility::{TreeScope_Children, UIA_ProcessIdPropertyId};
    use windows::Win32::System::Variant::VARIANT;
    
    // Get the process ID of the reference window
    let process_id = unsafe { window.CurrentProcessId() }?;
    log::debug!("[Sibling Search] Looking for windows with process ID: {}", process_id);
    
    // Create condition to match the same process ID
    let mut variant = VARIANT::default();
    unsafe {
        use windows::Win32::System::Variant::VARENUM;
        use std::mem::ManuallyDrop;
        
        let var_ptr = &mut variant as *mut VARIANT;
        let vt_ptr = var_ptr as *mut VARENUM;
        std::ptr::write(vt_ptr, windows::Win32::System::Variant::VT_I4);
        
        let int_ptr = (var_ptr as *mut u8).add(8) as *mut ManuallyDrop<i32>;
        std::ptr::write(int_ptr, ManuallyDrop::new(process_id as i32));
    }
    
    let condition = unsafe {
        auto.CreatePropertyCondition(
            UIA_ProcessIdPropertyId,
            &variant
        )
    }?;
    
    // Find all elements with this process ID from Desktop root
    // TreeScope_Children only searches immediate children (top-level windows)
    let desktop = unsafe { auto.GetRootElement() }?;
    let elements = unsafe {
        desktop.FindAll(
            TreeScope_Children,
            &condition
        )
    }?;
    
    let count = unsafe { elements.Length() }?;
    log::debug!("[Sibling Search] Found {} windows with same process ID", count);
    
    // Get the HWND of the original window for comparison
    let original_hwnd = unsafe { window.CurrentNativeWindowHandle() }?;
    
    let mut siblings = Vec::new();
    for i in 0..count {
        let elem = unsafe { elements.GetElement(i as i32) }?;
        
        // Skip the original window itself by comparing HWND
        let elem_hwnd = unsafe { elem.CurrentNativeWindowHandle() }?;
        if elem_hwnd.0 == original_hwnd.0 {
            continue;
        }
        
        siblings.push(elem);
    }
    
    log::debug!("[Sibling Search] Returning {} sibling windows (excluding original)", siblings.len());
    Ok(siblings)
}

pub(super) fn find_child_process_windows(
    auto: &IUIAutomation,
    parent_window: &IUIAutomationElement,
) -> anyhow::Result<Vec<IUIAutomationElement>> {
    use std::collections::HashSet;
    
    // Get parent process info
    let parent_pid = unsafe { parent_window.CurrentProcessId() }?;
    let parent_process_name = get_process_name_by_id(parent_pid as u32);
    log::info!("[Child Process Search] Parent PID={}, ProcessName='{}'", parent_pid, parent_process_name);
    
    // Enumerate all windows and filter by process name
    let mut candidate_windows = Vec::new();
    let mut related_pids: HashSet<u32> = HashSet::new();
    
    // Use EnumWindows to efficiently get all top-level windows
    let windows_list = crate::core::enum_windows::enumerate_windows_fast();
    log::info!("[Child Process Search] Found {} total windows via EnumWindows", windows_list.len());
    
    for win_info in windows_list.iter() {
        // Skip the parent process itself
        if win_info.process_id as i32 == parent_pid {
            continue;
        }
        
        // Skip if we've already seen this PID
        if !related_pids.contains(&win_info.process_id) {
            // Check if this process name is related to the parent
            let proc_name_lower = win_info.process_name.to_lowercase();
            let parent_name_lower = parent_process_name.to_lowercase();
            
            // Only match if process names are genuinely related:
            // 1. Exact match (e.g., "Weixin" == "Weixin")
            // 2. One starts with the other (e.g., "WeixinApp" starts with "Weixin")
            // 3. Known aliases (weixin <-> wechat)
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
        use windows::Win32::UI::Accessibility::TreeScope_Children;
        let desktop = unsafe { auto.GetRootElement() }?;
        let true_cond = unsafe { auto.CreateTrueCondition() }?;
        let all_windows = unsafe {
            desktop.FindAll(TreeScope_Children, &true_cond)
        }?;
        
        let count = unsafe { all_windows.Length() }?;
        for i in 0..count {
            if let Ok(elem) = unsafe { all_windows.GetElement(i as i32) } {
                if let Ok(elem_pid) = unsafe { elem.CurrentProcessId() } {
                    if related_pids.contains(&(elem_pid as u32)) {
                        let class_name = get_bstr(unsafe { elem.CurrentClassName() });
                        let hwnd = unsafe { elem.CurrentNativeWindowHandle() }.ok();
                        let is_visible = hwnd.map(|h| check_window_visibility(h).unwrap_or(false)).unwrap_or(false);
                        if is_visible {
                            log::info!("[Child Process Search] Adding UIA window: PID={}, Class='{}'", 
                                elem_pid, class_name);
                            candidate_windows.push(elem);
                        }
                    }
                }
            }
        }
    }
    
    log::info!("[Child Process Search] Found {} candidate windows from related processes", candidate_windows.len());
    Ok(candidate_windows)
}

fn check_window_visibility(hwnd: windows::Win32::Foundation::HWND) -> anyhow::Result<bool> {
    use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;
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
    
    unsafe extern "system" fn enum_callback(child: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let hwnds = &*(lparam.0 as *const std::cell::RefCell<Vec<HWND>>);
        // Skip invisible windows and cap at MAX_CHILD_HWNDS
        if !IsWindowVisible(child).as_bool() {
            return windows::core::BOOL(1); // Continue enumeration
        }
        let mut vec = hwnds.borrow_mut();
        if vec.len() >= MAX_CHILD_HWNDS {
            return windows::core::BOOL(0); // Stop enumeration
        }
        vec.push(child);
        windows::core::BOOL(1) // Continue enumeration
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

