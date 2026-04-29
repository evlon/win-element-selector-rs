// src/core/uia.rs
//
// Windows UI Automation core operations.
// Shared between GUI and HTTP API.
//
// XPath execution uses uiauto-xpath library for full XPath 1.0 standard support.

// Allow non-upper-case globals for UIA constants from windows crate.
#![allow(non_upper_case_globals)]

use super::model::{CaptureResult, DetailedValidationResult, ElementRect, HierarchyNode, LayerValidationResult, Operator, PropertyFilter, PropertyValidationResult, SegmentValidationResult, ValidationResult, WindowInfo};
use log::{debug, error, info};
use uiauto_xpath::{XPath, UiElement as UiaXPathElement};

// ═══════════════════════════════════════════════════════════════════════════════
// Windows implementation
// ═══════════════════════════════════════════════════════════════════════════════
#[cfg(target_os = "windows")]
pub mod windows_impl {
    use super::*;
    use windows::{
        core::BSTR,
        Win32::{
            Foundation::POINT,
            System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
            UI::{
                Accessibility::{
                    CUIAutomation, IUIAutomation, IUIAutomationElement,
                    IUIAutomationTreeWalker,
                },
                WindowsAndMessaging::GetCursorPos,
            },
        },
    };

    // Lazily created IUIAutomation instance (COM STA — must stay on UI thread).
    thread_local! {
        static AUTOMATION: std::cell::RefCell<Option<IUIAutomation>> =
            std::cell::RefCell::new(None);
    }

    fn get_automation() -> anyhow::Result<IUIAutomation> {
        AUTOMATION.with(|cell| {
            let mut opt = cell.borrow_mut();
            if opt.is_none() {
                let auto: IUIAutomation = unsafe {
                    CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                }
                .map_err(|e| anyhow::anyhow!("CoCreateInstance IUIAutomation: {e}"))?;
                *opt = Some(auto);
            }
            Ok(opt.as_ref().unwrap().clone())
        })
    }

    /// Capture the element under the mouse cursor.
    #[allow(dead_code)]
    pub fn capture_at_cursor() -> CaptureResult {
        let pt = unsafe {
            let mut p = POINT::default();
            if GetCursorPos(&mut p).is_err() {
                return CaptureResult {
                    hierarchy: vec![],
                    cursor_x: 0, cursor_y: 0,
                    error: Some("GetCursorPos 失败".to_string()),
                    window_info: None,
                };
            }
            p
        };
        capture_at_point(pt.x, pt.y)
    }

    /// Capture the element at a specific screen coordinate.
    pub fn capture_at_point(x: i32, y: i32) -> CaptureResult {
        match do_capture(x, y) {
            Ok(result) => result,
            Err(e) => {
                error!("capture_at_point({x},{y}) failed: {e}");
                CaptureResult {
                    hierarchy: vec![],
                    cursor_x: x, cursor_y: y,
                    error: Some(e.to_string()),
                    window_info: None,
                }
            }
        }
    }

    /// Get process name by process ID using Windows API.
    fn get_process_name_by_id(process_id: u32) -> String {
        #[cfg(target_os = "windows")]
        {
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
        }
        
        String::new()
    }

    /// Extract window information from the captured hierarchy.
    /// 窗口选择器必须定位顶层窗口（Desktop的直接子节点），
    /// 这样 find_window_by_selector 才能通过 TreeScope_Children 找到它。
    /// 
    /// hierarchy 结构（已 reverse 为 root → target）：
    /// - hierarchy[0] = Desktop
    /// - hierarchy[1] = 顶层窗口（Desktop的直接子节点）← 窗口选择器目标
    /// - hierarchy[2..last-1] = 中间层级节点
    /// - hierarchy[last] = 目标元素
    fn extract_window_info(hierarchy: &[HierarchyNode]) -> Option<WindowInfo> {
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

    fn do_capture(x: i32, y: i32) -> anyhow::Result<CaptureResult> {
        let auto = get_automation()?;
        let pt   = POINT { x, y };

        let target: IUIAutomationElement = unsafe {
            auto.ElementFromPoint(pt)
                .map_err(|e| anyhow::anyhow!("ElementFromPoint: {e}"))?
        };

        let mut chain: Vec<IUIAutomationElement> = vec![target.clone()];

        // Walk up ancestors (max 32 levels).
        let walker: IUIAutomationTreeWalker = unsafe {
            auto.ControlViewWalker()
                .map_err(|e| anyhow::anyhow!("ControlViewWalker: {e}"))?
        };

        let mut current = target.clone();
        for _ in 0..32 {
            let parent = unsafe { walker.GetParentElement(&current) };
            match parent {
                Ok(p) => {
                    // Null element means we've reached the root.
                    let is_null = unsafe { 
                        auto.CompareElements(&p, &auto.GetRootElement()?)
                            .unwrap_or(windows::core::BOOL(0))
                            .as_bool()
                    };
                    if is_null {
                        chain.push(p);
                        break;
                    }
                    chain.push(p.clone());
                    current = p;
                }
                Err(_) => break,
            }
        }

        chain.reverse(); // root → target

        let mut hierarchy: Vec<HierarchyNode> = Vec::with_capacity(chain.len());
        for elem in &chain {
            if let Some(node) = element_to_node(elem, &auto) {
                hierarchy.push(node);
            }
        }

        // Compute sibling index for the last element (target).
        if let Some(last) = hierarchy.last_mut() {
            last.index = sibling_index(&target, &walker).unwrap_or(0);
            if last.index > 0 {
                // Update the Index filter value.
                if let Some(f) = last.filters.iter_mut().find(|f| f.name == "Index") {
                    f.value   = last.index.to_string();
                    f.enabled = true;
                }
                // Compute total sibling count for last() function support
                last.sibling_count = count_siblings(&target, &walker).unwrap_or(0);
            }
        }

        // Extract window info before moving hierarchy.
        let window_info = extract_window_info(&hierarchy);

        Ok(CaptureResult {
            hierarchy,
            cursor_x: x,
            cursor_y: y,
            error: None,
            window_info,
        })
    }

    fn element_to_node(
        elem: &IUIAutomationElement,
        _auto: &IUIAutomation,
    ) -> Option<HierarchyNode> {
        let control_type = unsafe {
            elem.CurrentControlType()
                .map(control_type_name)
                .unwrap_or_default()
        };

        let automation_id = get_bstr(unsafe { elem.CurrentAutomationId() });
        let class_name    = get_bstr(unsafe { elem.CurrentClassName() });
        let name          = get_bstr(unsafe { elem.CurrentName() });
        let process_id    = unsafe { elem.CurrentProcessId().unwrap_or(0) as u32 };
        
        // Extract extended properties
        let framework_id = get_bstr(unsafe { elem.CurrentFrameworkId() });
        let help_text = get_bstr(unsafe { elem.CurrentHelpText() });
        let localized_control_type = get_bstr(unsafe { elem.CurrentLocalizedControlType() });
        let is_enabled = match unsafe { elem.CurrentIsEnabled() } {
            Ok(val) => val.as_bool(),
            Err(_) => true,
        };
        let is_offscreen = match unsafe { elem.CurrentIsOffscreen() } {
            Ok(val) => val.as_bool(),
            Err(_) => false,
        };
        
        // AccRole is deprecated in UIA, use ControlType instead
        // But we can still extract it if needed from LegacyIAccessible pattern
        let acc_role = String::new();

        let rect = unsafe {
            elem.CurrentBoundingRectangle()
                .map(|r| ElementRect {
                    x:      r.left,
                    y:      r.top,
                    width:  r.right  - r.left,
                    height: r.bottom - r.top,
                })
                .unwrap_or_default()
        };

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
        
        // Add extended property filters (only UIA standard properties, not human-readable ones)
        if !node.framework_id.is_empty() {
            node.filters.push(PropertyFilter::new("FrameworkId", &node.framework_id));
        }
        if !node.help_text.is_empty() {
            node.filters.push(PropertyFilter::new("HelpText", &node.help_text));
        }
        
        Some(node)
    }

    /// 1-based index among same-type siblings under the parent.
    fn sibling_index(
        target: &IUIAutomationElement,
        walker: &IUIAutomationTreeWalker,
    ) -> Option<i32> {
        let parent = unsafe { walker.GetParentElement(target).ok()? };
        let mut child = unsafe { walker.GetFirstChildElement(&parent).ok()? };
        let target_ct = unsafe { target.CurrentControlType().ok()? };

        let mut idx = 0i32;
        loop {
            let ct = unsafe { child.CurrentControlType().ok()? };
            if ct == target_ct {
                idx += 1;
            }
            // Compare by RuntimeId.
            let same = unsafe {
                let aid_child  = child.CurrentAutomationId().unwrap_or_default();
                let aid_target = target.CurrentAutomationId().unwrap_or_default();
                aid_child == aid_target
            };
            if same { return Some(idx); }
            match unsafe { walker.GetNextSiblingElement(&child) } {
                Ok(next) => child = next,
                Err(_)   => break,
            }
        }
        None
    }

    /// Count total siblings with the same ControlType under the parent.
    fn count_siblings(
        target: &IUIAutomationElement,
        walker: &IUIAutomationTreeWalker,
    ) -> Option<i32> {
        let parent = unsafe { walker.GetParentElement(target).ok()? };
        let mut child = unsafe { walker.GetFirstChildElement(&parent).ok()? };
        let target_ct = unsafe { target.CurrentControlType().ok()? };

        let mut count = 0i32;
        loop {
            let ct = unsafe { child.CurrentControlType().ok()? };
            if ct == target_ct {
                count += 1;
            }
            match unsafe { walker.GetNextSiblingElement(&child) } {
                Ok(next) => child = next,
                Err(_)   => break,
            }
        }
        Some(count)
    }

    fn get_bstr<T: Into<BSTR>>(r: windows::core::Result<T>) -> String {
        r.ok()
            .map(|b| {
                let bstr: BSTR = b.into();
                bstr.to_string()
            })
            .unwrap_or_default()
    }

    fn control_type_name(id: windows::Win32::UI::Accessibility::UIA_CONTROLTYPE_ID) -> String {
        use windows::Win32::UI::Accessibility::*;
        match id {
            UIA_ButtonControlTypeId       => "Button",
            UIA_CalendarControlTypeId     => "Calendar",
            UIA_CheckBoxControlTypeId     => "CheckBox",
            UIA_ComboBoxControlTypeId     => "ComboBox",
            UIA_CustomControlTypeId       => "Custom",
            UIA_DataGridControlTypeId     => "DataGrid",
            UIA_DataItemControlTypeId     => "DataItem",
            UIA_DocumentControlTypeId     => "Document",
            UIA_EditControlTypeId         => "Edit",
            UIA_GroupControlTypeId        => "Group",
            UIA_HeaderControlTypeId       => "Header",
            UIA_HeaderItemControlTypeId   => "HeaderItem",
            UIA_HyperlinkControlTypeId    => "Hyperlink",
            UIA_ImageControlTypeId        => "Image",
            UIA_ListControlTypeId         => "List",
            UIA_ListItemControlTypeId     => "ListItem",
            UIA_MenuBarControlTypeId      => "MenuBar",
            UIA_MenuControlTypeId         => "Menu",
            UIA_MenuItemControlTypeId     => "MenuItem",
            UIA_PaneControlTypeId         => "Pane",
            UIA_ProgressBarControlTypeId  => "ProgressBar",
            UIA_RadioButtonControlTypeId  => "RadioButton",
            UIA_ScrollBarControlTypeId    => "ScrollBar",
            UIA_SemanticZoomControlTypeId => "SemanticZoom",
            UIA_SeparatorControlTypeId    => "Separator",
            UIA_SliderControlTypeId       => "Slider",
            UIA_SpinnerControlTypeId      => "Spinner",
            UIA_SplitButtonControlTypeId  => "SplitButton",
            UIA_StatusBarControlTypeId    => "StatusBar",
            UIA_TabControlTypeId          => "Tab",
            UIA_TabItemControlTypeId      => "TabItem",
            UIA_TableControlTypeId        => "Table",
            UIA_TextControlTypeId         => "Text",
            UIA_ThumbControlTypeId        => "Thumb",
            UIA_TitleBarControlTypeId     => "TitleBar",
            UIA_ToolBarControlTypeId      => "ToolBar",
            UIA_ToolTipControlTypeId      => "ToolTip",
            UIA_TreeControlTypeId         => "Tree",
            UIA_TreeItemControlTypeId     => "TreeItem",
            UIA_WindowControlTypeId       => "Window",
            _                             => "Control",
        }.to_string()
    }

    // ── Validation ────────────────────────────────────────────────────────────

    /// Validate using window selector and element XPath (new format).
    /// Stage 1: Parse window selector to find target window
    /// Stage 2: Search elements using element XPath inside the window
    /// Returns detailed validation result with per-segment information.
    pub fn validate_selector_and_xpath_detailed(
        window_selector: &str,
        element_xpath: &str,
        hierarchy: &[HierarchyNode],
    ) -> DetailedValidationResult {
        use std::time::Instant;
        let total_start = Instant::now();
        
        let auto = match get_automation() {
            Ok(a)  => a,
            Err(e) => {
                return DetailedValidationResult {
                    overall: ValidationResult::Error(e.to_string()),
                    segments: vec![],
                    layers: vec![],
                    total_duration_ms: total_start.elapsed().as_millis() as u64,
                };
            }
        };

        // Stage 1: Find target window using window selector
        log::info!("[XPath Validation] Stage 1/2: Locating window with selector: {}", window_selector);
        
        let search_root = match find_window_by_selector(&auto, window_selector) {
            Some(window) => {
                log::info!("[XPath Validation] ✓ Window found, searching inside window");
                window
            }
            None => {
                return DetailedValidationResult {
                    overall: ValidationResult::Error(
                        format!("窗口未找到: {}", window_selector)
                    ),
                    segments: vec![],
                    layers: vec![],
                    total_duration_ms: total_start.elapsed().as_millis() as u64,
                };
            }
        };

        log::info!("[XPath Validation] Stage 2/2: Searching elements with XPath: {}", element_xpath);
        
        // Debug: 打印窗口的直接子节点，以及第一个子节点的子节点
        log::info!("[XPath Validation] Window's direct children:");
        let walker = unsafe { auto.ControlViewWalker().ok() };        
        if let Some(walker) = walker {
            let mut child = unsafe { walker.GetFirstChildElement(&search_root).ok() };
            let mut idx = 0;
            while let Some(c) = child {
                let ct = unsafe { c.CurrentControlType().map(control_type_name).unwrap_or_default() };                
                let class = get_bstr(unsafe { c.CurrentClassName() });
                let name = get_bstr(unsafe { c.CurrentName() });
                let fwid = get_bstr(unsafe { c.CurrentFrameworkId() });
                log::info!("  child[{}] {} class='{}' name='{}' frameworkId='{}'", idx, ct, class, name, fwid);
                        
                // 打印第一个子节点的子节点（检查 WRY_WEBVIEW 的内部结构）                
                if idx == 0 {
                    log::info!("  WRY_WEBVIEW's children:");
                    let mut sub_child = unsafe { walker.GetFirstChildElement(&c).ok() };
                    let mut sub_idx = 0;
                    while let Some(sc) = sub_child {
                        let sub_ct = unsafe { sc.CurrentControlType().map(control_type_name).unwrap_or_default() };                        
                        let sub_class = get_bstr(unsafe { sc.CurrentClassName() });
                        let sub_name = get_bstr(unsafe { sc.CurrentName() });
                        let sub_fwid = get_bstr(unsafe { sc.CurrentFrameworkId() });
                        log::info!("    sub[{}] {} class='{}' name='{}' frameworkId='{}'", sub_idx, sub_ct, sub_class, sub_name, sub_fwid);
                        sub_child = unsafe { walker.GetNextSiblingElement(&sc).ok() };                        
                        sub_idx += 1;
                        if sub_idx > 5 { break; }
                    }
                }
                        
                child = unsafe { walker.GetNextSiblingElement(&c).ok() };                
                idx += 1;
                if idx > 10 { break; } // 最多打印10个
            }
        }
                
        // Debug: 查找目标元素的父节点并打印其子节点的 ClassName
        // 用于检查 predicate 匹配失败的原因
        log::info!("[XPath Validation] Debug: searching for parent group nodes...");
        
        // Stage 2: Search elements from search_root with detailed tracking
        
        let (results, segments) = match find_by_xpath_detailed(&auto, &search_root, element_xpath) {
            Ok((results, segments)) => (results, segments),
            Err(e) => {
                return DetailedValidationResult {
                    overall: ValidationResult::Error(e.to_string()),
                    segments: vec![],
                    layers: vec![],
                    total_duration_ms: total_start.elapsed().as_millis() as u64,
                };
            }
        };
        
        let overall = if results.is_empty() {
            ValidationResult::NotFound
        } else {
            let first_rect = results.first().and_then(|e| {
                unsafe { e.CurrentBoundingRectangle().ok() }.map(|r| ElementRect {
                    x: r.left, y: r.top,
                    width: r.right - r.left, height: r.bottom - r.top,
                })
            });
            ValidationResult::Found { count: results.len(), first_rect }
        };
        
        // 生成逐层校验结果（复用 uiauto-xpath 的 ancestors 和 get_property API）
        let layers = if !results.is_empty() {
            let first_match = UiaXPathElement::from_raw(results[0].clone(), auto.clone());
            let ancestors = first_match.ancestors();
            
            hierarchy.iter().enumerate().map(|(layer_idx, node)| {
                let actual_elem = if layer_idx == hierarchy.len() - 1 {
                    Some(&first_match)
                } else {
                    let ancestor_idx = ancestors.len().saturating_sub(layer_idx + 1);
                    ancestors.get(ancestor_idx)
                };
                
                match actual_elem {
                    Some(elem) => {
                        let props: Vec<PropertyValidationResult> = node.filters.iter().map(|f| {
                            let actual = elem.get_property(&f.name);
                            let matched = actual.as_ref().map_or(false, |act| {
                                match f.operator {
                                    Operator::Equals => act == &f.value,
                                    Operator::NotEquals => act != &f.value,
                                    Operator::Contains => act.contains(&f.value),
                                    Operator::NotContains => !act.contains(&f.value),
                                    Operator::StartsWith => act.starts_with(&f.value),
                                    Operator::NotStartsWith => !act.starts_with(&f.value),
                                    Operator::EndsWith => act.ends_with(&f.value),
                                    Operator::NotEndsWith => !act.ends_with(&f.value),
                                    _ => act == &f.value,
                                }
                            });
                            PropertyValidationResult {
                                attr_name: f.name.clone(),
                                operator: f.operator.clone(),
                                expected_value: f.value.clone(),
                                actual_value: actual,
                                matched,
                                enabled: f.enabled,
                            }
                        }).collect();
                        let all_matched = props.iter().all(|p| p.matched || !p.enabled);
                        LayerValidationResult {
                            node_index: layer_idx,
                            control_type: node.control_type.clone(),
                            node_label: node.tree_label(),
                            matched: all_matched,
                            properties: props,
                            match_count: 1,
                            duration_ms: 0,
                        }
                    }
                    None => LayerValidationResult {
                        node_index: layer_idx,
                        control_type: node.control_type.clone(),
                        node_label: node.tree_label(),
                        matched: false,
                        properties: node.filters.iter().map(|f| PropertyValidationResult {
                            attr_name: f.name.clone(),
                            operator: f.operator.clone(),
                            expected_value: f.value.clone(),
                            actual_value: None,
                            matched: false,
                            enabled: f.enabled,
                        }).collect(),
                        match_count: 0,
                        duration_ms: 0,
                    },
                }
            }).collect()
        } else {
            vec![]
        };
        
        DetailedValidationResult {
            overall,
            segments,
            layers,
            total_duration_ms: total_start.elapsed().as_millis() as u64,
        }
    }

    /// Enumerate all top-level windows on desktop.
    /// Uses feature-based filtering instead of hardcoded class names.
    /// 
    /// Filtering features:
    /// 1. Must be Window/Pane/Application control type
    /// 2. Must have non-empty title
    /// 3. Must have sufficient size (>= 100x100 pixels)
    /// 4. Must not be shell system windows (class name pattern)
    /// 5. Must not be UI sub-components (class name contains Host/View/List)
    pub fn enumerate_windows() -> Vec<WindowInfo> {
        let auto = match get_automation() {
            Ok(a) => a,
            Err(_) => return vec![],
        };

        let desktop = match unsafe { auto.GetRootElement() } {
            Ok(d) => d,
            Err(_) => return vec![],
        };

        use windows::Win32::UI::Accessibility::*;
        let condition = match unsafe { auto.CreateTrueCondition() } {
            Ok(c) => c,
            Err(_) => return vec![],
        };

        let windows = match unsafe { desktop.FindAll(TreeScope_Descendants, &condition) } {
            Ok(w) => w,
            Err(_) => return vec![],
        };

        let count = match unsafe { windows.Length() } {
            Ok(c) => {
                debug!("enumerate_windows: found {} total elements (TreeScope_Descendants)", c);
                c
            },
            Err(_) => return vec![],
        };

        let mut window_list = Vec::new();
        for i in 0..count {
            let win = match unsafe { windows.GetElement(i) } {
                Ok(w) => w,
                Err(_) => continue,
            };

            let ct = unsafe {
                win.CurrentControlType()
                    .map(control_type_name)
                    .unwrap_or_default()
            };

            // 支持多种窗口类型：Window, Pane, Application
            // 这些都可能是应用的主窗口
            let valid_window_types = ["Window", "Pane", "Application"];
            
            if valid_window_types.contains(&ct.as_str()) {
                let title = get_bstr(unsafe { win.CurrentName() });
                let class = get_bstr(unsafe { win.CurrentClassName() });
                let pid = unsafe { win.CurrentProcessId().unwrap_or(0) as u32 };

                // Only include windows with non-empty title
                if !title.is_empty() {
                    // Get window rect for size checking
                    let rect = unsafe {
                        win.CurrentBoundingRectangle()
                            .map(|r| (r.right - r.left, r.bottom - r.top))
                            .unwrap_or((0, 0))
                    };
                    // Feature-based filtering (instead of hardcoded class names)
                    // Feature 1: Size check - skip small windows (tooltips, menus)
                    if rect.0 < 100 || rect.1 < 100 {
                        continue;
                    }
                    
                    // Feature 2: Check if it's a shell system window
                    // Pattern: class name starts with "Shell" (Shell_TrayWnd, ShellTabWindowClass, etc.)
                    // or equals Progman/WorkerW (desktop components)
                    let is_shell_window = class.starts_with("Shell") 
                        || class == "Progman" 
                        || class == "WorkerW";
                    if is_shell_window {
                        continue;
                    }
                    
                    // Feature 3: Check if it's a UI sub-component
                    // Pattern: class name contains common sub-control keywords
                    // - Host (ProperTreeHost, etc.)
                    // - View (BrowserRootView, but not Chrome_WidgetWin which is main window)
                    // - List (DUIListView, etc.)
                    // - Tab (ShellTabWindowClass, tab controls)
                    // - Tip (TeachingTip, tooltips)
                    // - Starts with Windows.UI/Microsoft.UI (UWP system windows)
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

    /// Activate (bring to front) a window by selector.
    /// Returns true if successful, false if window not found or activation failed.
    /// 
    /// Uses UI Automation SetFocus() to activate the window element.
    pub fn activate_window_by_selector(window_selector: &str) -> bool {
        debug!("Activating window: {}", window_selector);
        
        let auto = match get_automation() {
            Ok(a) => a,
            Err(_) => return false,
        };

        // Find the window element
        let window_element = match find_window_by_selector(&auto, window_selector) {
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

    /// Activate window and set focus to a specific element.
    /// First activates the window, then focuses the element.
    pub fn activate_and_focus_element(window_selector: &str, xpath: &str) -> bool {
        debug!("Activating window and focusing element: {} / {}", window_selector, xpath);
        
        let auto = match get_automation() {
            Ok(a) => a,
            Err(_) => return false,
        };

        // Find and activate the window
        let window_element = match find_window_by_selector(&auto, window_selector) {
            Some(w) => w,
            None => return false,
        };

        // Activate window via SetFocus
        unsafe {
            if window_element.SetFocus().is_err() {
                return false;
            }
        }
        
        // Small delay for window activation
        std::thread::sleep(std::time::Duration::from_millis(100));
        
        // Find target element using find_by_xpath_detailed
        match find_by_xpath_detailed(&auto, &window_element, xpath) {
            Ok((elements, _)) if !elements.is_empty() => {
                // Focus the first matching element
                unsafe {
                    elements[0].SetFocus().ok().is_some()
                }
            }
            _ => false,
        }
    }


    /// Find target window by parsing window selector XPath.
    /// Example: "Window[@Name='微信' and @ClassName='mmui::MainWindow']"
    fn find_window_by_selector(
        auto: &IUIAutomation,
        window_selector: &str,
    ) -> Option<IUIAutomationElement> {
        use windows::Win32::UI::Accessibility::*;
        
        // Parse window selector to extract conditions
        let (expected_name, expected_class) = parse_window_selector(window_selector);
        
        let desktop = unsafe { auto.GetRootElement().ok()? };
        let condition = unsafe { auto.CreateTrueCondition().ok()? };
        let windows = unsafe { desktop.FindAll(TreeScope_Children, &condition).ok()? };
        
        let count = unsafe { windows.Length().ok()? };
        
        // 支持多种窗口类型：Window, Pane, Document 等
        let valid_window_types = ["Window", "Pane", "Document", "Application"];
        
        for i in 0..count {
            let win = unsafe { windows.GetElement(i).ok()? };
            let ct = unsafe {
                win.CurrentControlType()
                    .map(control_type_name)
                    .unwrap_or_default()
            };
            
            // 支持多种窗口类型
            if valid_window_types.contains(&ct.as_str()) {
                let title = get_bstr(unsafe { win.CurrentName() });
                let class = get_bstr(unsafe { win.CurrentClassName() });
                
                // Match by parsed conditions
                let name_match = expected_name.as_ref().map_or(true, |n| &title == n);
                let class_match = expected_class.as_ref().map_or(true, |c| &class == c);
                
                if name_match && class_match {
                    return Some(win);
                }
            }
        }
        
        None
    }

    /// Parse window selector to extract Name and ClassName conditions.
    /// Returns (Option<Name>, Option<ClassName>)
    fn parse_window_selector(selector: &str) -> (Option<String>, Option<String>) {
        let mut name = None;
        let mut class = None;
        
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
            }
        }
        
        (name, class)
    }

    /// Find elements by XPath with detailed per-segment validation results.
    /// Uses uiauto-xpath library for full XPath 1.0 standard support.
    fn find_by_xpath_detailed(
        auto: &IUIAutomation,
        root: &IUIAutomationElement,
        xpath: &str,
    ) -> anyhow::Result<(Vec<IUIAutomationElement>, Vec<SegmentValidationResult>)> {
        use std::time::Instant;

        let total_start = Instant::now();
        info!("[XPath Validation] Executing XPath with uiauto-xpath: {}", xpath);

        // Wrap the root element for uiauto-xpath
        let uia_elem = UiaXPathElement::new(root.clone(), auto.clone());

        // Compile and execute XPath using uiauto-xpath library
        let compiled_xpath = match XPath::compile(xpath) {
            Ok(xp) => xp,
            Err(e) => {
                error!("[XPath Validation] XPath compilation failed: {}", e);
                return Err(anyhow::anyhow!("XPath compilation error: {}", e));
            }
        };

        // Execute the query
        let matches: Vec<IUIAutomationElement> = match compiled_xpath.select_nodes(&uia_elem) {
            Ok(nodes) => {
                // Extract raw IUIAutomationElement from each UiElement
                // Note: This requires uiauto-xpath to expose raw_element() method
                // See: https://github.com/your-repo/uiauto-xpath
                nodes.into_iter()
                    .map(|n| {
                        // Use Clone to get the underlying raw element
                        // Since UiElement::new takes IUIAutomationElement and we have Clone,
                        // we need to access the raw field
                        // For now, we work around by creating a wrapper
                        let raw = n.raw_element().clone();
                        raw
                    })
                    .collect()
            },
            Err(e) => {
                error!("[XPath Validation] XPath execution failed: {}", e);
                return Err(anyhow::anyhow!("XPath execution error: {}", e));
            }
        };

        let total_duration_ms = total_start.elapsed().as_millis() as u64;
        info!("[XPath Validation] Found {} matches ({}ms total)", matches.len(), total_duration_ms);

        // Generate segment validation results for UI display
        // Since uiauto-xpath executes the entire XPath at once, we generate
        // a summary result instead of per-segment results
        let segment_results = vec![
            SegmentValidationResult {
                segment_index: 0,
                segment_text: xpath.to_string(),
                matched: !matches.is_empty(),
                match_count: matches.len(),
                duration_ms: total_duration_ms,
                predicate_failures: Vec::new(), // 暂时为空，后续实现收集
            }
        ];

        Ok((matches, segment_results))
    }

    /// Find all matching elements and return their detailed info
    /// Used for findAll API
    pub fn find_all_elements_detailed(
        window_selector: &str,
        element_xpath: &str,
        random_range: f32,
    ) -> Vec<crate::api::types::ElementInfo> {
        use crate::api::types::{ElementInfo, Rect, Point};
        use rand::Rng;
        
        let auto = get_automation().ok().unwrap();
        let window_element = find_window_by_selector(&auto, window_selector);
        
        if window_element.is_none() {
            return vec![];
        }
        
        let window = window_element.unwrap();
        let (elements, _) = match find_by_xpath_detailed(&auto, &window, element_xpath) {
            Ok(result) => result,
            Err(_) => return vec![],
        };
        
        let mut rng = rand::thread_rng();
        
        elements.iter().filter_map(|elem| {
            let rect = unsafe { elem.CurrentBoundingRectangle().ok() };
            if rect.is_none() {
                return None;
            }
            let r = rect.unwrap();
            let api_rect = Rect {
                x: r.left,
                y: r.top,
                width: r.right - r.left,
                height: r.bottom - r.top,
            };
            let center = api_rect.center();
            
            // Calculate random center
            let half_range_w = api_rect.width as f32 * random_range / 2.0;
            let half_range_h = api_rect.height as f32 * random_range / 2.0;
            let offset_x = rng.gen_range(-half_range_w..half_range_w) as i32;
            let offset_y = rng.gen_range(-half_range_h..half_range_h) as i32;
            let center_random = Point::new(center.x + offset_x, center.y + offset_y);
            
            Some(ElementInfo {
                rect: api_rect,
                center,
                center_random,
                control_type: unsafe { elem.CurrentControlType().map(control_type_name).unwrap_or_default() },
                name: get_bstr(unsafe { elem.CurrentName() }),
                is_enabled: unsafe { elem.CurrentIsEnabled().map(|b| b.as_bool()).unwrap_or(true) },
            })
        }).collect()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Non-Windows mock (rich demo data)
// ═══════════════════════════════════════════════════════════════════════════════
#[cfg(not(target_os = "windows"))]
pub mod mock_impl {
    use super::*;

    pub fn capture_at_cursor() -> CaptureResult { mock() }
    pub fn capture_at_point(_x: i32, _y: i32) -> CaptureResult { mock() }

    pub fn enumerate_windows() -> Vec<WindowInfo> {
        // Mock: return sample windows
        vec![
            WindowInfo {
                title: "示例窗口 1".to_string(),
                class_name: "MockWindow".to_string(),
                process_id: 1001,
                process_name: "mock_app1".to_string(),
            },
            WindowInfo {
                title: "示例窗口 2".to_string(),
                class_name: "MockWindow".to_string(),
                process_id: 1002,
                process_name: "mock_app2".to_string(),
            },
        ]
    }

    pub fn validate_selector_and_xpath_detailed(
        _window_selector: &str,
        _element_xpath: &str,
        _hierarchy: &[HierarchyNode],
    ) -> DetailedValidationResult {
        DetailedValidationResult {
            overall: ValidationResult::Found {
                count: 1,
                first_rect: Some(ElementRect { x: 200, y: 300, width: 120, height: 30 }),
            },
            segments: vec![
                SegmentValidationResult {
                    segment_index: 0,
                    segment_text: "//Button".to_string(),
                    matched: true,
                    match_count: 1,
                    duration_ms: 10,
                    predicate_failures: Vec::new(),
                }
            ],
            layers: vec![],
            total_duration_ms: 50,
        }
    }

    pub fn find_all_elements_detailed(
        _window_selector: &str,
        _element_xpath: &str,
        _random_range: f32,
    ) -> Vec<crate::api::types::ElementInfo> {
        use crate::api::types::{ElementInfo, Rect, Point};
        vec![
            ElementInfo {
                rect: Rect { x: 200, y: 300, width: 120, height: 30 },
                center: Point { x: 260, y: 315 },
                center_random: Point { x: 262, y: 317 },
                control_type: "Button".to_string(),
                name: "Mock Button".to_string(),
                is_enabled: true,
            }
        ]
    }
}

// ─── Public API (platform-agnostic) ──────────────────────────────────────────────

#[cfg(target_os = "windows")]
pub use windows_impl::*;

#[cfg(not(target_os = "windows"))]
pub use mock_impl::*;

// ─── Rich mock data ──────────────────────────────────────────────────────────

pub fn mock() -> CaptureResult {
    CaptureResult {
        hierarchy: vec![
            HierarchyNode::new(
                "Window", "MainAppWindow", "WpfWindow", "My Application  —  文档1",
                0, ElementRect { x: 0, y: 0, width: 1280, height: 800 }, 12345,
            ),
            HierarchyNode::new(
                "Pane", "", "DockPanel", "", 0,
                ElementRect { x: 0, y: 30, width: 1280, height: 770 }, 12345,
            ),
            HierarchyNode::new(
                "ToolBar", "mainToolbar", "ToolBarTray", "主工具栏", 0,
                ElementRect { x: 0, y: 30, width: 1280, height: 36 }, 12345,
            ),
            HierarchyNode::new(
                "Button", "btnSave", "Button", "保存", 2,
                ElementRect { x: 120, y: 34, width: 80, height: 28 }, 12345,
            ),
        ],
        cursor_x: 160,
        cursor_y: 48,
        error: None,
        window_info: Some(WindowInfo {
            title: "My Application  —  文档1".to_string(),
            class_name: "WpfWindow".to_string(),
            process_id: 12345,
            process_name: "MyApp".to_string(),
        }),
    }
}