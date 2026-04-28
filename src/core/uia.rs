// src/core/uia.rs
//
// Windows UI Automation core operations.
// Shared between GUI and HTTP API.

// Allow non-upper-case globals for UIA constants from windows crate.
#![allow(non_upper_case_globals)]

use super::model::{CaptureResult, DetailedValidationResult, ElementRect, HierarchyNode, PropertyFilter, SegmentValidationResult, ValidationResult, WindowInfo};
use log::{debug, error};

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
    /// Finds the first Window node in the chain (root → target).
    fn extract_window_info(hierarchy: &[HierarchyNode]) -> Option<WindowInfo> {
        // 跳过桌面节点，找到第一个真正的应用窗口节点
        // #32769 是 Windows 桌面窗口类名，Desktop 是桌面控制类型
        hierarchy
            .iter()
            .find(|n| {
                // 跳过桌面相关节点
                n.class_name != "#32769" 
                && n.control_type != "Desktop"
                && !n.name.starts_with("桌面")
            })
            .map(|w| {
                // Get process name from process_id
                let process_name = get_process_name_by_id(w.process_id);
                WindowInfo {
                    title: w.name.clone(),
                    class_name: w.class_name.clone(),
                    process_id: w.process_id,
                    process_name,
                }
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
                            .unwrap_or(windows::Win32::Foundation::BOOL(0))
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
    ) -> DetailedValidationResult {
        use std::time::Instant;
        let total_start = Instant::now();
        
        let auto = match get_automation() {
            Ok(a)  => a,
            Err(e) => {
                return DetailedValidationResult {
                    overall: ValidationResult::Error(e.to_string()),
                    segments: vec![],
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
                    total_duration_ms: total_start.elapsed().as_millis() as u64,
                };
            }
        };

        // Stage 2: Search elements from search_root with detailed tracking
        log::info!("[XPath Validation] Stage 2/2: Searching elements with XPath: {}", element_xpath);
        
        let (results, segments) = match find_by_xpath_detailed(&auto, &search_root, element_xpath) {
            Ok((results, segments)) => (results, segments),
            Err(e) => {
                return DetailedValidationResult {
                    overall: ValidationResult::Error(e.to_string()),
                    segments: vec![],
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
        
        DetailedValidationResult {
            overall,
            segments,
            total_duration_ms: total_start.elapsed().as_millis() as u64,
        }
    }

    /// Enumerate all top-level windows on desktop.
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

            // 支持多种窗口类型：Window, Pane, Document, Application 等
            // 这些都可能是应用的主窗口
            let valid_window_types = ["Window", "Pane", "Application"];
            
            if valid_window_types.contains(&ct.as_str()) {
                let title = get_bstr(unsafe { win.CurrentName() });
                let class = get_bstr(unsafe { win.CurrentClassName() });
                let pid = unsafe { win.CurrentProcessId().unwrap_or(0) as u32 };

                // Only include windows with non-empty title
                if !title.is_empty() {
                    // 排除不需要的窗口类名
                    let excluded_classes = [
                        "Shell_TrayWnd",      // 任务栏
                        "Progman",            // Program Manager
                        "MSTaskSwWClass",     // 任务切换
                        "WorkerW",            // 背景桌面 worker
                        "Shell_SecondaryTrayWnd", // 第二任务栏
                        "Windows.UI.Core.CoreWindow", // UWP后台窗口
                        "Microsoft.UI.Xaml.Controls.TeachingTip", // 提示框
                    ];
                    
                    if !excluded_classes.contains(&class.as_str()) {
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

    /// XPath segment with scope information.
    #[derive(Debug)]
    struct XPathSegment {
        /// Whether to search descendants (//) or just children (/).
        descendants: bool,
        /// Tag name (e.g., "Button", "Window").
        tag: String,
        /// Predicates (e.g., [("AutomationId", "=", "btn1")]).
        preds: Vec<(String, String, String)>,
    }

    /// Parse XPath into segments, respecting both / and // semantics.
    fn parse_xpath(xpath: &str) -> Vec<XPathSegment> {
        let mut segments = Vec::new();
        let mut remaining = xpath.trim();
        
        // 记录开头的 / 或 //，用于第一个segment的 descendants 设置
        let first_is_descendants = if remaining.starts_with("//") {
            remaining = &remaining[2..];
            true
        } else if remaining.starts_with('/') {
            remaining = &remaining[1..];
            false  // 单个 / 表示直接子节点
        } else {
            true  // 没有前缀，默认 descendant search
        };
        
        while !remaining.is_empty() {
            // Determine if next segment is // or /
            let descendants = if remaining.starts_with("//") {
                remaining = &remaining[2..];
                true
            } else if remaining.starts_with('/') {
                remaining = &remaining[1..];
                false
            } else if segments.is_empty() {
                // 第一个 segment 使用开头记录的 / 或 //
                first_is_descendants
            } else {
                // Subsequent segments without prefix default to children
                false
            };
            
            // Extract segment content (until next / or end)
            let end_pos = remaining
                .find('/')
                .unwrap_or(remaining.len());
            let seg_content = &remaining[..end_pos].trim();
            remaining = &remaining[end_pos..];
            
            if !seg_content.is_empty() {
                let (tag, preds) = parse_segment(seg_content);
                segments.push(XPathSegment {
                    descendants,
                    tag,
                    preds,
                });
            }
        }
        
        segments
    }

    /// Find elements by XPath with detailed per-segment validation results.
    fn find_by_xpath_detailed(
        auto: &IUIAutomation,
        root: &IUIAutomationElement,
        xpath: &str,
    ) -> anyhow::Result<(Vec<IUIAutomationElement>, Vec<SegmentValidationResult>)> {
        use std::time::Instant;
        use windows::Win32::UI::Accessibility::*;

        let segments = parse_xpath(xpath);
        log::info!("[XPath Validation] Parsing {} segments", segments.len());

        if segments.is_empty() {
            return Ok((vec![], vec![]));
        }

        // Start search from root element.
        let mut current_search_root = root.clone();
        let mut segment_results: Vec<SegmentValidationResult> = Vec::new();
        
        for (seg_idx, seg) in segments.iter().enumerate() {
            let seg_start = Instant::now();
            let seg_text = format!(
                "{}{}[{}]",
                if seg.descendants { "//" } else { "/" },
                seg.tag,
                seg.preds.iter()
                    .map(|(attr, op, val)| format!("@{}{}'{}'", attr, op, val))
                    .collect::<Vec<_>>()
                    .join(" and ")
            );
            
            log::info!(
                "[XPath Validation] Segment {}: {} tag='{}', preds={:?}",
                seg_idx,
                if seg.descendants { "//" } else { "/" },
                seg.tag,
                seg.preds
            );
            
            // Choose search scope based on segment type.
            let scope = if seg.descendants {
                TreeScope_Subtree
            } else {
                TreeScope_Children
            };
            
            let condition: IUIAutomationCondition = unsafe {
                auto.CreateTrueCondition()?
            };
            let found = unsafe {
                current_search_root.FindAll(scope, &condition)?
            };

            let count = unsafe { found.Length()? };
            log::info!("[XPath Validation] Searching {} elements", count);
            
            let mut matches: Vec<IUIAutomationElement> = Vec::new();
            
            for i in 0..count {
                let elem = unsafe { found.GetElement(i)? };
                let ct = unsafe {
                    elem.CurrentControlType()
                        .map(control_type_name)
                        .unwrap_or_default()
                };
                if !seg.tag.is_empty() && seg.tag != "*" && ct != seg.tag {
                    continue;
                }
                // Check predicates (skip Index).
                let all_match = seg.preds.iter().filter(|(attr, _, _)| attr != "Index" && attr != "ProcessName").all(|(attr, op, val)| {
                    let actual = match attr.as_str() {
                        "AutomationId"         => get_bstr(unsafe { elem.CurrentAutomationId() }),
                        "ClassName"            => get_bstr(unsafe { elem.CurrentClassName() }),
                        "Name"                 => get_bstr(unsafe { elem.CurrentName() }),
                        "ControlType"          => unsafe { elem.CurrentControlType().map(control_type_name).unwrap_or_default() },
                        "FrameworkId"          => get_bstr(unsafe { elem.CurrentFrameworkId() }),
                        "LocalizedControlType" => get_bstr(unsafe { elem.CurrentLocalizedControlType() }),
                        "HelpText"             => get_bstr(unsafe { elem.CurrentHelpText() }),
                        "IsEnabled"            => unsafe { elem.CurrentIsEnabled().map(|b| b.as_bool().to_string()).unwrap_or_default() },
                        "IsOffscreen"          => unsafe { elem.CurrentIsOffscreen().map(|b| b.as_bool().to_string()).unwrap_or_default() },
                        _                       => String::new(),
                    };
                    check_predicate(&actual, op, val)
                });
                if all_match {
                    matches.push(elem);
                }
            }
            
            let duration_ms = seg_start.elapsed().as_millis() as u64;
            log::info!("[XPath Validation] Found {} matches for segment {} ({}ms)", matches.len(), seg_idx, duration_ms);
            
            // Record segment result
            segment_results.push(SegmentValidationResult {
                segment_index: seg_idx,
                segment_text: seg_text,
                matched: !matches.is_empty(),
                match_count: matches.len(),
                duration_ms,
            });
            
            if matches.is_empty() {
                return Ok((vec![], segment_results));
            }
            
            // If this is the last segment, return all matches.
            if seg_idx == segments.len() - 1 {
                log::info!("[XPath Validation] Final result: {} matches", matches.len());
                return Ok((matches, segment_results));
            }
            
            // For next segment, use the first match as search root.
            current_search_root = matches[0].clone();
        }

        // Should never reach here, but just in case.
        log::info!("[XPath Validation] Final result: 0 matches");
        Ok((vec![], segment_results))
    }

    fn parse_segment(seg: &str) -> (String, Vec<(String, String, String)>) {
        if let Some(bracket) = seg.find('[') {
            let tag   = seg[..bracket].trim().to_string();
            let inner = seg[bracket + 1..seg.rfind(']').unwrap_or(seg.len())].trim();
            let preds = parse_predicates(inner);
            (tag, preds)
        } else {
            (seg.trim().to_string(), vec![])
        }
    }

    /// Very lightweight predicate parser for `@Attr op 'val'` and `contains(...)`.
    fn parse_predicates(s: &str) -> Vec<(String, String, String)> {
        let mut result = Vec::new();
        for part in s.split(" and ") {
            let part = part.trim();
            if part.starts_with("contains(") {
                // contains(@Attr, 'val')
                let inner = &part[9..part.rfind(')').unwrap_or(part.len())];
                let mut parts = inner.splitn(2, ',');
                let attr = parts.next().unwrap_or("").trim().trim_start_matches('@').to_string();
                let val  = parts.next().unwrap_or("").trim().trim_matches('\'').to_string();
                result.push((attr, "contains".to_string(), val));
            } else if part.starts_with("starts-with(") {
                let inner = &part[12..part.rfind(')').unwrap_or(part.len())];
                let mut parts = inner.splitn(2, ',');
                let attr = parts.next().unwrap_or("").trim().trim_start_matches('@').to_string();
                let val  = parts.next().unwrap_or("").trim().trim_matches('\'').to_string();
                result.push((attr, "starts-with".to_string(), val));
            } else if let Some(eq_pos) = part.find("!=") {
                let attr = part[..eq_pos].trim().trim_start_matches('@').to_string();
                let val  = part[eq_pos + 2..].trim().trim_matches('\'').to_string();
                result.push((attr, "!=".to_string(), val));
            } else if let Some(eq_pos) = part.find('=') {
                let attr = part[..eq_pos].trim().trim_start_matches('@').to_string();
                let val  = part[eq_pos + 1..].trim().trim_matches('\'').to_string();
                result.push((attr, "=".to_string(), val));
            }
        }
        result
    }

    fn check_predicate(actual: &str, op: &str, expected: &str) -> bool {
        match op {
            "="           => actual == expected,
            "!="          => actual != expected,
            "contains"    => actual.contains(expected),
            "starts-with" => actual.starts_with(expected),
            _             => false,
        }
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
                }
            ],
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