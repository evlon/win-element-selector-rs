// src/core/uia.rs
//
// Windows UI Automation core operations.
// Shared between GUI and HTTP API.
//
// XPath execution uses uiauto-xpath library for full XPath 1.0 standard support.

// Allow non-upper-case globals for UIA constants from windows crate.
#![allow(non_upper_case_globals)]

use super::model::{CaptureResult, DetailedValidationResult, ElementRect, HierarchyNode, LayerValidationResult, Operator, PredicateFailure, PropertyValidationResult, SegmentValidationResult, ValidationResult, WindowInfo};
use log::{debug, error, info};
use uiauto_xpath::{XPath, UiElement as UiaXPathElement, control_type_id_to_name, control_type_name_to_id};

// ═══════════════════════════════════════════════════════════════════════════════
// Windows implementation
// ═══════════════════════════════════════════════════════════════════════════════
pub mod windows_impl {
    use super::*;
    use std::collections::HashSet;
    use windows::{
        core::BSTR,
        Win32::{
            Foundation::{POINT, HWND, LPARAM, RECT},
            System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
            UI::{
                Accessibility::{
                    CUIAutomation, IUIAutomation, IUIAutomationElement,
                    IUIAutomationTreeWalker, TreeScope_Ancestors,
                    TreeScope_Subtree,
                },
                WindowsAndMessaging::{
                    GetCursorPos, EnumChildWindows, EnumWindows, GetWindowThreadProcessId,
                    IsWindowVisible,
                },
            },
        },
    };

    /// Check if a screen point is within a bounding rectangle (inclusive of edges).
    #[inline]
    fn point_in_rect(x: i32, y: i32, r: &RECT) -> bool {
        x >= r.left && x <= r.right && y >= r.top && y <= r.bottom
    }

    /// 计算元素的 visibleRect（元素矩形 ∩ 窗口视口矩形）
    /// 
    /// # Arguments
    /// * `element_rect` - 元素的边界矩形
    /// * `window_rect` - 窗口（视口）的边界矩形，如果为 None 则认为整个元素可见
    /// 
    /// # Returns
    /// 可见的矩形区域，如果完全不可见则返回 None
    fn compute_element_visible_rect(
        element_rect: &crate::api::types::Rect,
        window_rect: Option<&crate::api::types::Rect>,
    ) -> Option<crate::api::types::Rect> {
        match window_rect {
            Some(vp) => {
                let left = element_rect.x.max(vp.x);
                let top = element_rect.y.max(vp.y);
                let right = (element_rect.x + element_rect.width).min(vp.x + vp.width);
                let bottom = (element_rect.y + element_rect.height).min(vp.y + vp.height);
                if right > left && bottom > top {
                    Some(crate::api::types::Rect { 
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

    /// Build a dedup key from an element's RuntimeId.
    fn runtime_id_key(elem: &IUIAutomationElement) -> Option<Vec<i32>> {
        unsafe {
            let variant = elem.GetRuntimeId().ok()?;
            let len = (*variant).rgsabound[0].cElements as usize;
            let ptr = (*variant).pvData as *const i32;
            if ptr.is_null() || len == 0 {
                return None;
            }
            Some(std::slice::from_raw_parts(ptr, len).to_vec())
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // COM Management Layer - Unified COM lifecycle management
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// COM 线程模型状态
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum ComApartmentType {
        /// 未初始化
        Uninitialized,
        /// STA (Single-Threaded Apartment) - UIA 需要
        Sta,
        /// MTA (Multi-Threaded Apartment) - 不兼容 UIA
        Mta,
    }

    // /// COM 管理器 - 统一的 COM 生命周期管理
    // pub struct ComManager;

    // impl ComManager {
    //     /// 检查当前线程的 COM 状态
    //     pub fn check_current_apartment() -> ComApartmentType {
    //         use windows::Win32::System::Com::{CoGetApartmentType, APTTYPE, APTTYPEQUALIFIER};
            
    //         let mut apt_type = APTTYPE::default();
    //         let mut qualifier = APTTYPEQUALIFIER::default();
            
    //         match unsafe { CoGetApartmentType(&mut apt_type, &mut qualifier) } {
    //             Ok(_) => {
    //                 match apt_type {
    //                     APTTYPE(0) => ComApartmentType::Sta,  // APTTYPE_STA
    //                     APTTYPE(1) => ComApartmentType::Mta,  // APTTYPE_MTA
    //                     _ => ComApartmentType::Uninitialized,
    //                 }
    //             }
    //             Err(_) => ComApartmentType::Uninitialized,
    //         }
    //     }
        
    //     /// 确保当前线程处于 STA 模式
    //     /// 
    //     /// 返回结果：
    //     /// - Ok(true): 成功初始化或已是 STA
    //     /// - Ok(false): 已在 MTA 模式，无法切换（需要警告）
    //     /// - Err: 初始化失败
    //     pub fn ensure_sta() -> anyhow::Result<bool> {
    //         let current = Self::check_current_apartment();
            
    //         match current {
    //             ComApartmentType::Sta => {
    //                 log::debug!("COM already in STA mode");
    //                 Ok(true)
    //             }
    //             ComApartmentType::Mta => {
    //                 log::warn!("Thread is in MTA mode, UIA operations may fail!");
    //                 log::warn!("Consider using a dedicated STA thread for UI Automation");
    //                 Ok(false)
    //             }
    //             ComApartmentType::Uninitialized => {
    //                 Self::init_sta()
    //             }
    //         }
    //     }
        
    //     /// 初始化 COM STA 模式
    //     fn init_sta() -> anyhow::Result<bool> {
    //         use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
            
    //         let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            
    //         // HRESULT 值：
    //         // S_OK (0x00000000) = 首次初始化成功
    //         // S_FALSE (0x00000001) = 已经初始化
    //         // RPC_E_CHANGED_MODE (0x80010106) = 已在 MTA 模式
            
    //         if hr == windows::core::HRESULT(0) {
    //             log::debug!("COM STA initialized successfully");
    //             Ok(true)
    //         } else if hr == windows::core::HRESULT(1) {
    //             log::debug!("COM already initialized (S_FALSE)");
    //             Ok(true)
    //         } else if hr == windows::core::HRESULT(0x80010106u32 as i32) {
    //             log::error!("Cannot switch from MTA to STA! Thread already in MTA mode.");
    //             Ok(false)
    //         } else {
    //             Err(anyhow::anyhow!(
    //                 "CoInitializeEx failed with HRESULT={:#010x}", 
    //                 hr.0 as u32
    //             ))
    //         }
    //     }
        
    //     /// 安全地重新初始化 COM（用于检测到状态失效时）
    //     pub fn safe_reinitialize() -> anyhow::Result<()> {
    //         use windows::Win32::System::Com::{CoUninitialize, CoInitializeEx, COINIT_APARTMENTTHREADED};
            
    //         // 先卸载
    //         unsafe { CoUninitialize() };
    //         log::debug!("COM uninitialized for reinitialization");
            
    //         // 短暂等待，确保清理完成
    //         std::thread::sleep(std::time::Duration::from_millis(10));
            
    //         // 重新初始化
    //         let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            
    //         if hr == windows::core::HRESULT(0) || hr == windows::core::HRESULT(1) {
    //             log::info!("COM reinitialized successfully");
    //             Ok(())
    //         } else {
    //             Err(anyhow::anyhow!(
    //                 "COM reinitialization failed: HRESULT={:#010x}", 
    //                 hr.0 as u32
    //             ))
    //         }
    //     }
    // }

    /// 带有健康检查的 IUIAutomation 提供者
    pub struct AutomationProvider;

    impl AutomationProvider {
        /// 获取 IUIAutomation 实例，带健康检查
        pub fn get_healthy() -> anyhow::Result<IUIAutomation> {
            AUTOMATION.with(|cell| {
                let mut opt = cell.borrow_mut();
                
                // 检查现有实例是否有效
                if let Some(ref auto) = *opt {
                    // 尝试一个简单的操作来验证实例是否仍然有效
                    if Self::validate_instance(auto) {
                        log::debug!("Reusing existing IUIAutomation instance");
                        return Ok(auto.clone());
                    } else {
                        log::warn!("Existing IUIAutomation instance is invalid, recreating...");
                        *opt = None;
                    }
                }
                
                // 创建新实例
                log::debug!("Creating new IUIAutomation instance");
                let auto: IUIAutomation = unsafe {
                    CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                }
                .map_err(|e| anyhow::anyhow!("CoCreateInstance IUIAutomation: {e}"))?;
                
                *opt = Some(auto.clone());
                Ok(auto)
            })
        }
        
        /// 验证 IUIAutomation 实例是否有效
        fn validate_instance(auto: &IUIAutomation) -> bool {
            use std::time::Instant;
            
            // 尝试获取根元素作为健康检查，并设置超时
            let start = Instant::now();
            let result = unsafe {
                auto.GetRootElement()
            };
            let elapsed = start.elapsed();
            
            // 如果操作超过 100ms，认为 COM 对象已经失效
            if elapsed.as_millis() > 100 {
                log::warn!("IUIAutomation health check took {}ms (too slow, likely stale)", 
                          elapsed.as_millis());
                return false;
            }
            
            result.is_ok()
        }
        
        /// 强制重置 IUIAutomation 实例
        pub fn force_reset() {
            AUTOMATION.with(|cell| {
                let mut opt = cell.borrow_mut();
                *opt = None;
                log::debug!("IUIAutomation instance reset");
            });
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Legacy API - Backward compatibility
    // ═══════════════════════════════════════════════════════════════════════════

    // Lazily created IUIAutomation instance (COM STA — must stay on UI thread).
    thread_local! {
        static AUTOMATION: std::cell::RefCell<Option<IUIAutomation>> =
            std::cell::RefCell::new(None);
    }

    /// Legacy function - Use AutomationProvider::get_healthy() instead
    fn get_automation() -> anyhow::Result<IUIAutomation> {
        AutomationProvider::get_healthy()
    }

    /// Initialize COM in STA (Single-Threaded Apartment) mode for UI Automation.
    /// Must be called on each `spawn_blocking` thread before any UIA operation.
    /// 
    /// Deprecated: Use ComManager::ensure_sta() instead
    // pub fn ensure_com_sta() -> anyhow::Result<()> {
    //     ComManager::ensure_sta().map(|_| ())
    // }

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
                    error: Some(format!("捕获失败: {}", e)),
                    window_info: None,
                }
            }
        }
    }

    /// Get process name by process ID using Windows API.
    fn get_process_name_by_id(process_id: u32) -> String {
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
        info!("[Normal] Starting capture at ({}, {})", x, y);

        // 1. Get the element at the point
        let target: IUIAutomationElement = unsafe {
            auto.ElementFromPoint(pt)
                .map_err(|e| anyhow::anyhow!("ElementFromPoint: {e}"))?
        };
        let target_name = get_bstr(unsafe { target.CurrentName() });
        let target_ct = unsafe { target.CurrentControlType().map(control_type_name).unwrap_or_default() };
        debug!("[Normal] ElementFromPoint: type='{}' name='{}'", target_ct, target_name);

        // 2. Get full ancestor chain using FindAll(TreeScope_Ancestors)
        let condition = unsafe {
            auto.CreateTrueCondition()
                .map_err(|e| anyhow::anyhow!("CreateTrueCondition: {e}"))?
        };
        let ancestors = unsafe {
            target.FindAll(TreeScope_Ancestors, &condition)
                .map_err(|e| anyhow::anyhow!("FindAll(Ancestors): {e}"))?
        };

        let ancestor_count = unsafe { ancestors.Length()? };
        debug!("[Normal] FindAll(Ancestors) returned {} elements", ancestor_count);

        let mut chain: Vec<IUIAutomationElement> = Vec::with_capacity(ancestor_count as usize + 1);
        chain.push(target.clone());
        for i in 0..ancestor_count {
            if let Ok(elem) = unsafe { ancestors.GetElement(i) } {
                chain.push(elem);
            }
        }

        // Chain is: [target, parent, grandparent, ..., root]
        chain.reverse(); // now: root → target

        // 3. Build hierarchy (root → target order)
        let window_index = 1;

        let mut hierarchy: Vec<HierarchyNode> = Vec::with_capacity(chain.len());
        for (chain_idx, elem) in chain.iter().enumerate() {
            if let Some(mut node) = element_to_node(elem, &auto) {
                node.depth_from_window = chain_idx.saturating_sub(window_index);
                hierarchy.push(node);
            }
        }

        // 4. Compute sibling index for the target element
        if let Some(last) = hierarchy.last_mut() {
            last.is_target = true;
            let walker = unsafe { auto.ControlViewWalker().ok() };
            if let Some(ref w) = walker {
                last.index = sibling_index(&target, w).unwrap_or(0);
                if last.index > 0 {
                    if let Some(f) = last.filters.iter_mut().find(|f| f.name == "Index") {
                        f.value   = last.index.to_string();
                        f.enabled = true;
                    }
                    last.sibling_count = count_siblings(&target, w).unwrap_or(0);
                }
            }
        }

        let window_info = extract_window_info(&hierarchy);

        debug!("[Normal] Capture complete: hierarchy_depth={} target='{}'",
            hierarchy.len(), target_name);

        Ok(CaptureResult {
            hierarchy,
            cursor_x: x,
            cursor_y: y,
            error: None,
            window_info,
        })
    }

    /// Determine if a ControlType is inherently clickable (even without InvokePattern).
    fn is_control_type_clickable(control_type: &str) -> bool {
        matches!(control_type, "Button" | "Hyperlink" | "ListItem" | "MenuItem"
            | "TabItem" | "TreeItem" | "RadioButton" | "CheckBox"
            | "ComboBox" | "Link" | "Image" | "Document")
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
        let is_password = match unsafe { elem.CurrentIsPassword() } {
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
        node.is_password = is_password;
        node.accelerator_key = get_bstr(unsafe { elem.CurrentAcceleratorKey() });
        node.access_key = get_bstr(unsafe { elem.CurrentAccessKey() });
        node.item_type = get_bstr(unsafe { elem.CurrentItemType() });
        node.item_status = get_bstr(unsafe { elem.CurrentItemStatus() });

        // ─── UIA Pattern detection ───────────────────────────────────────────
        use windows::Win32::UI::Accessibility::{
            UIA_TogglePatternId, UIA_InvokePatternId, UIA_ScrollPatternId, UIA_SelectionItemPatternId,
        };
        let has_toggle = unsafe {
            elem.GetCurrentPattern(UIA_TogglePatternId).is_ok()
        };
        let has_invoke = unsafe {
            elem.GetCurrentPattern(UIA_InvokePatternId).is_ok()
        };
        let has_scroll = unsafe {
            elem.GetCurrentPattern(UIA_ScrollPatternId).is_ok()
        };
        let has_selection_item = unsafe {
            elem.GetCurrentPattern(UIA_SelectionItemPatternId).is_ok()
        };

        node.is_checkable = has_toggle;
        node.is_clickable = has_invoke || is_control_type_clickable(&control_type);
        node.is_scrollable = has_scroll;

        // Read ToggleState if checkable
        if has_toggle {
            if let Ok(pattern) = unsafe {
                elem.GetCurrentPattern(UIA_TogglePatternId)
            } {
                use windows::Win32::UI::Accessibility::IToggleProvider;
                let toggle: IToggleProvider = unsafe {
                    std::mem::transmute(pattern)
                };
                if let Ok(state) = unsafe { toggle.ToggleState() } {
                    // ToggleState_Off = 0, ToggleState_On = 1, ToggleState_Indeterminate = 2
                    node.is_checked = Some(state.0 == 1);
                }
            }
        }

        // Read SelectionItem IsSelected if available
        if has_selection_item {
            if let Ok(pattern) = unsafe {
                elem.GetCurrentPattern(UIA_SelectionItemPatternId)
            } {
                use windows::Win32::UI::Accessibility::ISelectionItemProvider;
                let sel: ISelectionItemProvider = unsafe {
                    std::mem::transmute(pattern)
                };
                if let Ok(selected) = unsafe { sel.IsSelected() } {
                    node.is_selected = Some(selected.as_bool());
                }
            }
        }
        
        // Build extended property filters from all populated fields
        node.build_extended_filters();
        
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
        control_type_id_to_name(id.0).to_string()
    }

    // ── Validation ────────────────────────────────────────────────────────────

    /// Validate using window selector and element XPath (new format).
    /// Stage 1: Parse window selector to find target window(s)
    /// Stage 2: For each matching window, try element XPath until one succeeds
    /// Returns detailed validation result with per-segment information.
    pub fn validate_selector_and_xpath_detailed(
        window_selector: &str,
        element_xpath: &str,
        hierarchy: &[HierarchyNode],
    ) -> DetailedValidationResult {
        use std::time::Instant;
        let total_start = Instant::now();
        
        log::info!("[PERF] Starting validation for window_selector='{}' xpath='{}'", window_selector, element_xpath);
        
        let auto = match get_automation() {
            Ok(a)  => a,
            Err(e) => {
                log::error!("[PERF] Failed to get automation instance: {}", e);
                return DetailedValidationResult {
                    overall: ValidationResult::Error(e.to_string()),
                    segments: vec![],
                    layers: vec![],
                    total_duration_ms: total_start.elapsed().as_millis() as u64,
                    is_offscreen: None,
                };
            }
        };

        // Stage 1: Find all target windows using window selector
        log::info!("[PERF] Stage 1/2: Locating window with selector: {}", window_selector);
        let stage1_start = Instant::now();
        
        let matched_windows = find_window_by_selector(&auto, window_selector);
        
        let stage1_duration = stage1_start.elapsed().as_millis();
        log::info!("[PERF] Stage 1 completed in {}ms, found {} windows", stage1_duration, matched_windows.len());
        
        if matched_windows.is_empty() {
            return DetailedValidationResult {
                overall: ValidationResult::Error(
                    format!("窗口未找到: {}", window_selector)
                ),
                segments: vec![],
                layers: vec![],
                total_duration_ms: total_start.elapsed().as_millis() as u64,
                is_offscreen: None,
            };
        }
        
        log::info!("[PERF] ✓ Found {} matching window(s), trying XPath on each", matched_windows.len());

        // Stage 2: Try XPath on each matching window, return first success
        // This handles multi-process scenarios (e.g., multiple Tauri app instances)
        let mut last_error: Option<String> = None;
        let mut best_result: Option<(Vec<IUIAutomationElement>, Vec<SegmentValidationResult>)> = None;

        for (win_idx, search_root) in matched_windows.iter().enumerate() {
            log::info!("[PERF] Stage 2/2: Trying XPath on window {} of {}", win_idx + 1, matched_windows.len());
            let stage2_window_start = Instant::now();

            // Debug-only: print window's direct children tree for diagnostics
            #[cfg(debug_assertions)]
            if win_idx == 0 {
                log::info!("[XPath Validation] Window's direct children:");
                let walker = unsafe { auto.ControlViewWalker().ok() };
                if let Some(walker) = walker {
                    let mut child = unsafe { walker.GetFirstChildElement(search_root).ok() };
                    let mut idx = 0;
                    while let Some(c) = child {
                        let ct = unsafe { c.CurrentControlType().map(control_type_name).unwrap_or_default() };
                        let class = get_bstr(unsafe { c.CurrentClassName() });
                        let name = get_bstr(unsafe { c.CurrentName() });
                        let fwid = get_bstr(unsafe { c.CurrentFrameworkId() });
                        log::info!("  child[{}] {} class='{}' name='{}' frameworkId='{}'", idx, ct, class, name, fwid);

                        if idx == 0 {
                            log::info!("  first child's sub-children:");
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
                        if idx > 10 { break; }
                    }
                }
            }

            match find_by_xpath_with_fallback(&auto, search_root, element_xpath) {
                Ok((results, segments)) => {
                    let window_duration = stage2_window_start.elapsed().as_millis();
                    if !results.is_empty() {
                        // Found! Use this window's results
                        log::info!("[PERF] ✓ Window {} XPath succeeded in {}ms, found {} results", win_idx + 1, window_duration, results.len());
                        best_result = Some((results, segments));
                        break;
                    }
                    // No match in this window, try next
                    log::info!("[PERF] Window {} - XPath matched 0 elements in {}ms, trying next window", win_idx + 1, window_duration);
                    if best_result.is_none() {
                        // Keep empty results as fallback
                        best_result = Some((results, segments));
                    }
                }
                Err(e) => {
                    let window_duration = stage2_window_start.elapsed().as_millis();
                    log::info!("[PERF] Window {} - XPath error in {}ms: {}, trying next window", win_idx + 1, window_duration, e);
                    last_error = Some(e.to_string());
                }
            }
        }

        let (results, segments) = match best_result {
            Some(r) => r,
            None => {
                return DetailedValidationResult {
                    overall: ValidationResult::Error(
                        last_error.unwrap_or_else(|| "XPath 执行失败".to_string())
                    ),
                    segments: vec![],
                    layers: vec![],
                    total_duration_ms: total_start.elapsed().as_millis() as u64,
                    is_offscreen: None,
                };
            }
        };
        
        let overall = if results.is_empty() {
            ValidationResult::NotFound
        } else {
            let mut rects = Vec::with_capacity(results.len());
            for elem in &results {
                if let Ok(r) = unsafe { elem.CurrentBoundingRectangle() } {
                    rects.push(ElementRect {
                        x: r.left, y: r.top,
                        width: r.right - r.left, height: r.bottom - r.top,
                    });
                }
            }
            let first_rect = rects.first().cloned();
            ValidationResult::Found { count: results.len(), first_rect, rects }
        };

        // 查询第一个匹配元素的 isOffscreen
        let is_offscreen = if !results.is_empty() {
            Some(unsafe { results[0].CurrentIsOffscreen() }
                .map(|b| b.as_bool())
                .unwrap_or(false))
        } else {
            None
        };
        
        // 生成逐层校验结果（复用 uiauto-xpath 的 ancestors 和 get_property API）
        let layers = if !results.is_empty() {
            let first_match = UiaXPathElement::new(results[0].clone(), auto.clone());
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
            is_offscreen,
        }
    }

    /// Enhanced capture: enumerate all descendants of the target window using RawViewWalker,
    /// then find the innermost element whose BoundingRectangle contains the cursor point.
    /// This solves the problem where ElementFromPoint returns a wrapper/container element
    /// (e.g., WebView-based apps) instead of the actual target element.
    pub fn capture_enhanced_at_point(x: i32, y: i32) -> CaptureResult {
        let auto = match get_automation() {
            Ok(a) => a,
            Err(e) => {
                return CaptureResult {
                    hierarchy: vec![], cursor_x: x, cursor_y: y,
                    error: Some(format!("COM 初始化失败: {}", e)),
                    window_info: None,
                };
            }
        };
        match do_capture_enhanced(&auto, x, y) {
            Ok(result) => result,
            Err(e) => {
                error!("capture_enhanced_at_point({},{}) failed: {}", x, y, e);
                CaptureResult {
                    hierarchy: vec![], cursor_x: x, cursor_y: y,
                    error: Some(format!("增强捕获失败: {}", e)),
                    window_info: None,
                }
            }
        }
    }

    fn do_capture_enhanced(auto: &IUIAutomation, x: i32, y: i32) -> anyhow::Result<CaptureResult> {
        let pt = POINT { x, y };
        debug!("[Enhanced] Starting at ({}, {})", x, y);

        let true_cond = unsafe { auto.CreateTrueCondition()? };

        // Step 1: ElementFromPoint to get the top-level element at cursor
        let hit_elem = unsafe {
            auto.ElementFromPoint(pt)
                .map_err(|e| anyhow::anyhow!("ElementFromPoint: {}", e))?
        };
        let hit_name = get_bstr(unsafe { hit_elem.CurrentName() });
        let hit_ct = unsafe { hit_elem.CurrentControlType().map(control_type_name).unwrap_or_default() };
        debug!("[Enhanced] ElementFromPoint: type='{}' name='{}'", hit_ct, hit_name);

        // Step 2: FindAll(TreeScope_Subtree) on the hit element to get ALL descendants
        let all_elements = unsafe { hit_elem.FindAll(TreeScope_Subtree, &true_cond)? };
        let element_count = unsafe { all_elements.Length()? };
        debug!("[Enhanced] FindAll(Subtree) returned {} elements", element_count);

        // Step 3: Iterate, filter by point-in-rect + offscreen + RuntimeId dedup, pick innermost
        let mut best_elem: Option<IUIAutomationElement> = None;
        let mut best_area = i64::MAX;
        let mut best_ct = String::new();
        let mut best_name = String::new();
        let mut best_rid_len: usize = 0;
        let mut best_index: i32 = 0;
        let mut seen_ids: HashSet<Vec<i32>> = HashSet::new();
        let mut point_match_count = 0;
        let mut offscreen_skip_count = 0;

        for i in 0..element_count {
            let elem = unsafe { all_elements.GetElement(i)? };

            // RuntimeId dedup
            let Some(rid) = runtime_id_key(&elem) else { continue };
            let rid_len = rid.len();
            if !seen_ids.insert(rid) { continue; }

            // IsOffscreen filter
            if let Ok(is_offscreen) = unsafe { elem.CurrentIsOffscreen() } {
                if is_offscreen.0 != 0 { offscreen_skip_count += 1; continue; }
            }

            // BoundingRectangle must contain the point
            let rect = match unsafe { elem.CurrentBoundingRectangle() } {
                Ok(r) => r,
                Err(_) => continue,
            };
            let w = rect.right - rect.left;
            let h = rect.bottom - rect.top;
            if w <= 0 || h <= 0 || !point_in_rect(x, y, &rect) { continue; }

            point_match_count += 1;
            let area = w as i64 * h as i64;
            let e_ct = unsafe { elem.CurrentControlType().map(control_type_name).unwrap_or_default() };
            let e_name = get_bstr(unsafe { elem.CurrentName() });
            debug!("[Enhanced]   match #{}: type='{}' name='{}' area={} rid_len={} rect=[{},{},{}x{}]",
                point_match_count, e_ct, e_name, area, rid_len, rect.left, rect.top, w, h);

            // Pick innermost: RuntimeId length (depth proxy) > smallest area > array index (later = deeper)
            let dominated = rid_len > best_rid_len
                || (rid_len == best_rid_len && area < best_area)
                || (rid_len == best_rid_len && area == best_area && i > best_index);
            if dominated {
                best_area = area;
                best_ct = e_ct;
                best_name = e_name;
                best_elem = Some(elem);
                best_rid_len = rid_len;
                best_index = i;
            }
        }

        debug!("[Enhanced] filtered — point_match={}, offscreen_skip={}",
            point_match_count, offscreen_skip_count);

        let target_elem = best_elem
            .ok_or_else(|| anyhow::anyhow!("没有找到包含光标位置的元素"))?;
        debug!("[Enhanced] SELECTED: type='{}' name='{}' area={} rid_len={}", best_ct, best_name, best_area, best_rid_len);
        debug!("[Enhanced] COMPARISON: normal→type='{}' name='{}' | enhanced→type='{}' name='{}'",
            hit_ct, hit_name, best_ct, best_name);

        // Step 4: Build ancestor chain
        let target_ancestors = unsafe { target_elem.FindAll(TreeScope_Ancestors, &true_cond)? };
        let ancestor_count = unsafe { target_ancestors.Length()? };

        let mut chain: Vec<IUIAutomationElement> = Vec::with_capacity(ancestor_count as usize + 1);
        for i in (0..ancestor_count).rev() {
            if let Ok(e) = unsafe { target_ancestors.GetElement(i) } {
                chain.push(e);
            }
        }
        chain.push(target_elem.clone());

        // Fallback: FindAll(Ancestors) returns 0 for elements outside Control view (e.g. WebView).
        // Fall back to ControlViewWalker loop.
        if ancestor_count == 0 {
            debug!("[Enhanced] FindAll(Ancestors) returned 0, falling back to walker");
            chain.clear();
            let walker = unsafe { auto.ControlViewWalker()? };
            let desktop = unsafe { auto.GetRootElement()? };

            let mut elements: Vec<IUIAutomationElement> = vec![target_elem.clone()];
            let mut current = unsafe { walker.GetParentElement(&target_elem).ok() };
            while let Some(elem) = current {
                let is_desktop = unsafe { auto.CompareElements(&elem, &desktop).unwrap_or(windows::core::BOOL(0)).as_bool() };
                elements.push(elem.clone());
                if is_desktop {
                    break;
                }
                current = unsafe { walker.GetParentElement(&elem).ok() };
            }
            elements.reverse();
            chain = elements;
            debug!("[Enhanced] walker chain length = {}", chain.len());
        }

        // Step 5: Build hierarchy (same structure as normal capture)
        let window_index = 1;
        let mut hierarchy: Vec<HierarchyNode> = Vec::with_capacity(chain.len());
        for (chain_idx, elem) in chain.iter().enumerate() {
            if let Some(mut node) = element_to_node(elem, &auto) {
                node.depth_from_window = chain_idx.saturating_sub(window_index);
                debug!("[Enhanced]   hierarchy[{}]: type='{}' name='{}' depth={}",
                    chain_idx, node.control_type, node.name, node.depth_from_window);
                hierarchy.push(node);
            }
        }

        let walker_control = unsafe { auto.ControlViewWalker()? };
        if let Some(last) = hierarchy.last_mut() {
            last.is_target = true;
            last.index = sibling_index(&target_elem, &walker_control).unwrap_or(0);
            if last.index > 0 {
                if let Some(f) = last.filters.iter_mut().find(|f| f.name == "Index") {
                    f.value = last.index.to_string(); f.enabled = true;
                }
                last.sibling_count = count_siblings(&target_elem, &walker_control).unwrap_or(0);
            }
        }

        let window_info = extract_window_info(&hierarchy);
        info!("[Enhanced] hierarchy depth={} target='{}' vs normal type='{}' name='{}'",
            hierarchy.len(),
            hierarchy.last().map(|n| &n.control_type).unwrap_or(&String::new()),
            hit_ct, hit_name);

        Ok(CaptureResult { hierarchy, cursor_x: x, cursor_y: y, error: None, window_info })
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
                let pid = unsafe { win.CurrentProcessId().ok() }.unwrap_or(0) as u32;

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

    /// Check if a window matching the selector exists.
    /// Returns true if at least one matching window is found.
    /// Reuses find_window_by_selector for the search logic.
    pub fn exists_window_by_selector(window_selector: &str) -> bool {
        debug!("Checking window existence: {}", window_selector);
        
        let auto = match get_automation() {
            Ok(a) => a,
            Err(_) => return false,
        };

        let windows = find_window_by_selector(&auto, window_selector);
        !windows.is_empty()
    }

    /// Activate (bring to front) a window by selector.
    /// Returns true if successful, false if window not found or activation failed.
    /// 
    /// Uses UI Automation SetFocus() to activate the window element.
    /// When multiple windows match the selector, activates the first one.
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

    /// Activate window and set focus to a specific element.
    /// First activates the window, then focuses the element.
    /// When multiple windows match the selector, tries each until one succeeds.
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
                    // Focus the first matching element
                    if unsafe { elements[0].SetFocus() }.ok().is_some() {
                        return true;
                    }
                }
            }
        }
        
        false
    }

    /// 获取窗口的 BoundingRectangle
    /// 根据窗口选择器查找窗口，返回其边界矩形
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


    /// Find target windows by parsing window selector XPath.
    /// Example: "Window[@Name='微信' and @ClassName='mmui::MainWindow' and @ProcessName='Weixin']"
    /// Returns all matching windows (for multi-process scenario, try XPath on each).
    ///
    /// Performance strategy:
    /// 1. Fast path: Win32 EnumWindows + ElementFromHandle (milliseconds)
    ///    - EnumWindows only returns ~20-50 top-level windows (not all UIA elements)
    ///    - GetWindowThreadProcessId is a single syscall (much faster than OpenProcess+Query)
    ///    - ElementFromHandle converts HWND → UIA element in O(1)
    /// 2. Fallback: UIA condition-based search (if fast path fails)
    fn find_window_by_selector(
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

    /// Fast path: use Win32 EnumWindows to find matching windows.
    /// This bypasses slow UIA enumeration entirely.
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

    /// Enumerate all top-level windows using Win32 EnumWindows API.
    /// Returns a list of HWND handles for visible windows.
    fn enumerate_top_level_windows() -> Vec<HWND> {
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

    /// Fallback: use UIA condition-based search for windows.
    /// Used when EnumWindows fast path finds no matches (rare edge case).
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

    /// Build a UIA condition for window search, combining Name/ClassName property conditions.
    /// Returns CreateTrueCondition() as fallback if property condition creation fails.
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

    /// Parse window selector to extract Name, ClassName, and ProcessName conditions.
    /// Returns (Option<Name>, Option<ClassName>, Option<ProcessName>)
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

    /// Find the content root inside a window for XPath searching.
    /// 
    /// Some apps (e.g., Tauri/WRY) have intermediate panes between the window root
    /// and the actual content (e.g., "Intermediate D3D Window" / "WRY_WEBVIEW" wrapping Chrome WebView).
    /// This function detects such intermediate panes and returns the best search root.
    /// 
    /// Detection strategies (in priority order):
    /// 1. Well-known WebView container class names (WRY_WEBVIEW, Chrome_WidgetWin_*, etc.)
    /// 2. Deep framework transition search (Win32 → Chrome at any depth)
    fn find_content_root(auto: &IUIAutomation, window: &IUIAutomationElement) -> Option<IUIAutomationElement> {
        let walker = unsafe { auto.ControlViewWalker().ok() }?;
        let window_fwid = get_bstr(unsafe { window.CurrentFrameworkId() });
        
        log::info!("[Content Root] Window FrameworkId='{}', scanning children...", window_fwid);
        
        // Well-known WebView/browser container class name prefixes
        // These are panes that wrap web content in various frameworks
        const WEBVIEW_CLASS_PREFIXES: &[&str] = &[
            "WRY_WEBVIEW",           // Tauri/WRY apps
            "Chrome_WidgetWin",      // Chromium-based (Electron, Edge, etc.)
            "Chrome_Widget",         // Shorter prefix for Chrome_WidgetWin_1, etc.
            "Intermediate D3D",      // D3D intermediate window (some WRY versions)
            "WebView2",              // WebView2 apps
            "CefBrowserWindow",      // CEF-based apps
            "QtWebEngine",           // Qt's WebEngine
            "QWebEngine",            // Qt WebEngine variant
        ];
        
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
            
            // Strategy 1: Match well-known WebView container class names
            let is_webview_container = WEBVIEW_CLASS_PREFIXES.iter()
                .any(|prefix| class.starts_with(prefix));
            if is_webview_container {
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
        
        log::info!("[Content Root] No content root found");
        None
    }

    /// Recursively check if any descendant of `elem` has a different FrameworkId.
    /// Uses iterative BFS to avoid stack overflow on deep UI trees.
    /// Optimized with early pruning and dynamic breadth limiting.
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

    /// Find all top-level windows belonging to the same process as the given window
    /// This is much faster than searching from Desktop root and handles multi-window scenarios
    fn find_sibling_windows_same_process(
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

    /// Find windows that might contain Chrome WebView content
    /// This handles multi-process applications like WeChat where WebView runs in a separate process
    fn find_child_process_windows(
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
    
    /// Helper function to check if a window is visible
    fn check_window_visibility(hwnd: windows::Win32::Foundation::HWND) -> anyhow::Result<bool> {
        use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;
        Ok(unsafe { IsWindowVisible(hwnd).as_bool() })
    }

    /// Find elements by XPath with automatic fallback for intermediate layers.
    /// 
    /// Strategy depends on XPath type:
    /// 
    /// For //XPath (descendant search — most common):
    /// 1. Try from content root first (fast — skips Win32 chrome/intermediate layers)
    /// 2. Fallback to window root if content root not found or yields no results
    /// 
    /// For /XPath (absolute path):
    /// 1. Try from window root first (most likely to match exact path)
    /// 2. Try from content root (direct path)
    /// 3. Try //XPath from content root (descendant within content)
    /// 4. Try //XPath from window root (last resort)
    fn find_by_xpath_with_fallback(
        auto: &IUIAutomation,
        window: &IUIAutomationElement,
        xpath: &str,
    ) -> anyhow::Result<(Vec<IUIAutomationElement>, Vec<SegmentValidationResult>)> {
        use std::time::Instant;
        let fallback_start = Instant::now();
        
        let is_descendant = xpath.starts_with("//");
        
        if is_descendant {
            // ── Descendant XPath (//...): prioritize content root ──
            // Content root has far fewer nodes than window root,
            // so descendant search is much faster from content root.
            
            // Step 1: Try content root first (fast path)
            if let Some(content_root) = find_content_root(auto, window) {
                log::info!("[XPath Fallback] //XPath — Step 1: content root (fast)");
                if let Ok((r, s)) = find_by_xpath_detailed(auto, &content_root, xpath) {
                    if !r.is_empty() {
                        log::info!("[XPath Fallback] ✓ Step 1: Found {} results from content root ({}ms)", 
                            r.len(), fallback_start.elapsed().as_millis());
                        return Ok((r, s));
                    }
                }
            }
            
            // Step 2: Fallback to window root
            log::info!("[XPath Fallback] //XPath — Step 2: window root (fallback)");
            let (results, segments) = find_by_xpath_detailed(auto, window, xpath)?;
            if !results.is_empty() {
                log::info!("[XPath Fallback] ✓ Step 2: Found {} results from window root ({}ms)", 
                    results.len(), fallback_start.elapsed().as_millis());
                return Ok((results, segments));
            }
            
            log::info!("[XPath Fallback] All //XPath fallbacks exhausted ({}ms)", 
                fallback_start.elapsed().as_millis());
            Ok((results, segments))
        } else {
            // ── Absolute XPath (/...): optimized multi-strategy approach ──
            
            // Strategy 1: Try from window root first (most common case, fastest)
            log::info!("[XPath Fallback] /XPath — Strategy 1: window root (primary)");
            let (results, segments) = find_by_xpath_detailed(auto, window, xpath)?;
            if !results.is_empty() {
                log::info!("[XPath Fallback] ✓ Strategy 1: Found {} from window root ({}ms)", 
                    results.len(), fallback_start.elapsed().as_millis());
                return Ok((results, segments));
            }
            
            // Strategy 2: Try content root if available (handles embedded WebView)
            log::info!("[XPath Fallback] /XPath — Strategy 2: trying content root...");
            if let Some(content_root) = find_content_root(auto, window) {
                // Try direct path from content root
                if let Ok((r2, s2)) = find_by_xpath_detailed(auto, &content_root, xpath) {
                    if !r2.is_empty() {
                        log::info!("[XPath Fallback] ✓ Strategy 2a: Found {} from content root ({}ms)", 
                            r2.len(), fallback_start.elapsed().as_millis());
                        return Ok((r2, s2));
                    }
                }
                
                // Try as descendant from content root
                let desc_xpath = format!("//{}", xpath.trim_start_matches('/'));
                log::info!("[XPath Fallback] /XPath — Strategy 2b: content root descendant");
                if let Ok((r3, s3)) = find_by_xpath_detailed(auto, &content_root, &desc_xpath) {
                    if !r3.is_empty() {
                        log::info!("[XPath Fallback] ✓ Strategy 2b: Found {} from content root desc ({}ms)", 
                            r3.len(), fallback_start.elapsed().as_millis());
                        return Ok((r3, s3));
                    }
                }
            }
            
            // Strategy 2.5: Use FindAll(TreeScope_Descendants) on the window to search the RAW tree.
            // This is critical for apps like WeChat where Chrome_Widget Pane exists in the raw tree
            // but is filtered out by ControlViewWalker (used by uiauto-xpath's children() method).
            // The capture uses RawViewWalker/FindAll(Subtree), so the XPath was generated against the raw tree,
            // but validation uses ControlViewWalker which can't see these elements.
            log::info!("[XPath Fallback] /XPath — Strategy 2.5: FindAll(Descendants) raw tree search");
            {
                let desc_xpath = format!("//{}", xpath.trim_start_matches('/'));
                if let Ok((r25, s25)) = find_by_xpath_raw_descendants(auto, window, &desc_xpath) {
                    if !r25.is_empty() {
                        log::info!("[XPath Fallback] ✓ Strategy 2.5: Found {} via raw descendant search ({}ms)", 
                            r25.len(), fallback_start.elapsed().as_millis());
                        return Ok((r25, s25));
                    }
                }
            }
            
            // Strategy 2.7: Search child HWNDs via EnumChildWindows.
            // Critical for apps like WeChat where Chrome_Widget Pane is under a child HWND
            // that's not visible as a direct child in the UIA tree from the main window.
            // The UIA tree can have an asymmetry: GetParentElement(child) → Window works,
            // but GetFirstChildElement(Window) → child doesn't return it.
            // EnumChildWindows bypasses this by enumerating Win32 child HWNDs directly.
            log::info!("[XPath Fallback] /XPath — Strategy 2.7: child HWND search via EnumChildWindows");
            if let Ok(hwnd) = unsafe { window.CurrentNativeWindowHandle() } {
                let child_hwnds = enum_child_hwnds(HWND(hwnd.0));
                log::info!("[Strategy 2.7] Found {} child HWNDs under window", child_hwnds.len());
                
                let desc_xpath = format!("//{}", xpath.trim_start_matches('/'));
                let xpath_parts: Vec<&str> = desc_xpath.split('/').filter(|s| !s.is_empty()).collect();
                
                for (idx, child_hwnd) in child_hwnds.iter().enumerate() {
                    if let Ok(child_elem) = unsafe { auto.ElementFromHandle(*child_hwnd) } {
                        let child_class = get_bstr(unsafe { child_elem.CurrentClassName() });
                        let child_ct = unsafe { child_elem.CurrentControlType().map(control_type_name).unwrap_or_default() };
                        let child_pid = unsafe { child_elem.CurrentProcessId().unwrap_or(0) };
                        log::info!("[Strategy 2.7]   child_hwnd[{}]: hwnd=0x{:X} type='{}' class='{}' pid={}",
                            idx, child_hwnd.0 as usize, child_ct, child_class, child_pid);
                        
                        // Try: if this child element itself matches the first step, search from it
                        if !xpath_parts.is_empty() {
                            let first_parsed = parse_xpath_step(xpath_parts[0]);
                            if element_matches_parsed_step(&child_elem, &first_parsed) {
                                log::info!("[Strategy 2.7]   ✓ child HWND matches first step!");
                                // Build remaining XPath after the first step
                                let remaining = if xpath_parts.len() > 1 {
                                    format!("/{}", xpath_parts[1..].join("/"))
                                } else {
                                    String::new()
                                };
                                
                                if remaining.is_empty() {
                                    // First step is the only step — the child element IS the match
                                    let duration_ms = fallback_start.elapsed().as_millis() as u64;
                                    log::info!("[XPath Fallback] ✓ Strategy 2.7: Found element via child HWND ({}ms)", duration_ms);
                                    return Ok((vec![child_elem], vec![SegmentValidationResult {
                                        segment_index: 0,
                                        segment_text: desc_xpath,
                                        matched: true,
                                        match_count: 1,
                                        duration_ms,
                                        predicate_failures: Vec::new(),
                                    }]));
                                }
                                
                                // Try uiauto-xpath from this element for the remaining path
                                if let Ok((matches, segments)) = find_by_xpath_detailed(auto, &child_elem, &remaining) {
                                    if !matches.is_empty() {
                                        log::info!("[XPath Fallback] ✓ Strategy 2.7: Found {} from child HWND subtree ({}ms)",
                                            matches.len(), fallback_start.elapsed().as_millis());
                                        let mut all_segments = vec![SegmentValidationResult {
                                            segment_index: 0,
                                            segment_text: xpath_parts[0].to_string(),
                                            matched: true,
                                            match_count: 1,
                                            duration_ms: 0,
                                            predicate_failures: Vec::new(),
                                        }];
                                        for mut s in segments {
                                            s.segment_index += 1;
                                            all_segments.push(s);
                                        }
                                        return Ok((matches, all_segments));
                                    }
                                }
                                
                                // Also try raw tree walk from this child
                                if let Ok(raw_walker) = unsafe { auto.RawViewWalker() } {
                                    let remaining_parts = &xpath_parts[1..];
                                    if let Ok(matches) = walk_raw_tree_steps(auto, &raw_walker, &child_elem, remaining_parts) {
                                        if !matches.is_empty() {
                                            log::info!("[XPath Fallback] ✓ Strategy 2.7: Found {} via child HWND raw walk ({}ms)",
                                                matches.len(), fallback_start.elapsed().as_millis());
                                            let segments: Vec<SegmentValidationResult> = xpath_parts.iter().enumerate().map(|(i, step)| {
                                                SegmentValidationResult {
                                                    segment_index: i,
                                                    segment_text: step.to_string(),
                                                    matched: i < xpath_parts.len() - 1 || !matches.is_empty(),
                                                    match_count: if i == xpath_parts.len() - 1 { matches.len() } else { 0 },
                                                    duration_ms: 0,
                                                    predicate_failures: Vec::new(),
                                                }
                                            }).collect();
                                            return Ok((matches, segments));
                                        }
                                    }
                                }
                            }
                            
                            // Also try: search inside this child HWND's subtree for the first step
                            if let Ok(raw_walker) = unsafe { auto.RawViewWalker() } {
                                let first_parsed = parse_xpath_step(xpath_parts[0]);
                                let mut sub_match = unsafe { raw_walker.GetFirstChildElement(&child_elem).ok() };
                                while let Some(sub) = sub_match {
                                    if element_matches_parsed_step(&sub, &first_parsed) {
                                        log::info!("[Strategy 2.7]   ✓ Found first-step match inside child HWND!");
                                        // Try remaining from this sub-match
                                        let remaining_parts = &xpath_parts[1..];
                                        if remaining_parts.is_empty() {
                                            let duration_ms = fallback_start.elapsed().as_millis() as u64;
                                            return Ok((vec![sub], vec![SegmentValidationResult {
                                                segment_index: 0,
                                                segment_text: desc_xpath,
                                                matched: true,
                                                match_count: 1,
                                                duration_ms,
                                                predicate_failures: Vec::new(),
                                            }]));
                                        }
                                        // Try uiauto-xpath then raw walk
                                        let remaining_xpath = format!("/{}", remaining_parts.join("/"));
                                        if let Ok((m, _)) = find_by_xpath_detailed(auto, &sub, &remaining_xpath) {
                                            if !m.is_empty() {
                                                let match_count = m.len();
                                                log::info!("[XPath Fallback] ✓ Strategy 2.7: Found {} inside child HWND via uiauto-xpath ({}ms)",
                                                    match_count, fallback_start.elapsed().as_millis());
                                                return Ok((m, vec![SegmentValidationResult {
                                                    segment_index: 0,
                                                    segment_text: desc_xpath,
                                                    matched: true,
                                                    match_count,
                                                    duration_ms: fallback_start.elapsed().as_millis() as u64,
                                                    predicate_failures: Vec::new(),
                                                }]));
                                            }
                                        }
                                        if let Ok(m) = walk_raw_tree_steps(auto, &raw_walker, &sub, remaining_parts) {
                                            if !m.is_empty() {
                                                let match_count = m.len();
                                                log::info!("[XPath Fallback] ✓ Strategy 2.7: Found {} inside child HWND via raw walk ({}ms)",
                                                    match_count, fallback_start.elapsed().as_millis());
                                                return Ok((m, vec![SegmentValidationResult {
                                                    segment_index: 0,
                                                    segment_text: desc_xpath,
                                                    matched: true,
                                                    match_count,
                                                    duration_ms: fallback_start.elapsed().as_millis() as u64,
                                                    predicate_failures: Vec::new(),
                                                }]));
                                            }
                                        }
                                    }
                                    sub_match = unsafe { raw_walker.GetNextSiblingElement(&sub).ok() };
                                }
                            }
                        }
                    }
                }
            } else {
                log::info!("[Strategy 2.7] Could not get window HWND, skipping");
            }
            
            // Strategy 3: Try sibling windows of the same process AND child processes (handles multi-process apps like WeChat)
            log::info!("[XPath Fallback] /XPath — Strategy 3: searching sibling windows and child processes...");
            if let Ok(siblings) = find_sibling_windows_same_process(auto, window) {
                log::info!("[XPath Fallback] Found {} sibling windows, trying XPath on each", siblings.len());
                for (idx, sibling) in siblings.iter().enumerate() {
                    if let Ok((r, s)) = find_by_xpath_detailed(auto, sibling, xpath) {
                        if !r.is_empty() {
                            log::info!("[XPath Fallback] ✓ Strategy 3: Found {} from sibling window {} ({}ms)", 
                                r.len(), idx + 1, fallback_start.elapsed().as_millis());
                            return Ok((r, s));
                        }
                    }
                }
            }
            
            // Strategy 3b: Try to find child process windows (for apps like WeChat with separate WebView processes)
            log::info!("[XPath Fallback] /XPath — Strategy 3b: searching child process windows...");
            if let Ok(child_windows) = find_child_process_windows(auto, window) {
                log::info!("[XPath Fallback] Found {} child process windows, trying XPath on each", child_windows.len());
                
                // Also try descendant search from child windows
                let desc_xpath = format!("//{}", xpath.trim_start_matches('/'));
                
                for (idx, child_win) in child_windows.iter().enumerate() {
                    // Get window info for debugging
                    let child_pid = unsafe { child_win.CurrentProcessId().unwrap_or(0) };
                    let child_name = get_bstr(unsafe { child_win.CurrentName() });
                    let child_class = get_bstr(unsafe { child_win.CurrentClassName() });
                    log::info!("[Strategy 3b] Trying window {}: PID={}, Class='{}', Name='{}'", 
                        idx + 1, child_pid, child_class, child_name);
                    
                    // Try to inspect the window's children structure
                    if let Ok(walker) = unsafe { auto.ControlViewWalker() } {
                        if let Some(first_child) = unsafe { walker.GetFirstChildElement(child_win).ok() } {
                            let child_name_str = get_bstr(unsafe { first_child.CurrentName() });
                            let child_class_inner = get_bstr(unsafe { first_child.CurrentClassName() });
                            let child_fwid = get_bstr(unsafe { first_child.CurrentFrameworkId() });
                            log::info!("  -> First child: class='{}', name='{}', frameworkId='{}'", 
                                child_class_inner, child_name_str, child_fwid);
                        } else {
                            log::info!("  -> No children found (empty UIA tree)");
                        }
                    }
                    
                    // Try absolute path first
                    if let Ok((r, s)) = find_by_xpath_detailed(auto, child_win, xpath) {
                        if !r.is_empty() {
                            log::info!("[XPath Fallback] ✓ Strategy 3b: Found {} from child process window {} (absolute path, {}ms)", 
                                r.len(), idx + 1, fallback_start.elapsed().as_millis());
                            return Ok((r, s));
                        }
                    }
                    
                    // Try descendant search as fallback
                    if let Ok((r, s)) = find_by_xpath_detailed(auto, child_win, &desc_xpath) {
                        if !r.is_empty() {
                            log::info!("[XPath Fallback] ✓ Strategy 3b: Found {} from child process window {} (descendant path, {}ms)", 
                                r.len(), idx + 1, fallback_start.elapsed().as_millis());
                            return Ok((r, s));
                        }
                    }
                }
            }
            
            // Strategy 4: Last resort — Desktop root descendant search (for deeply nested content like WeChat WebView)
            // This handles cases where content is embedded so deeply that window-level search fails
            log::info!("[XPath Fallback] /XPath — Strategy 4: Desktop root descendant (last resort for deep nesting)");
            let desktop_desc_xpath = format!("//{}", xpath.trim_start_matches('/'));
            if let Ok(desktop) = unsafe { auto.GetRootElement() } {
                // Use a timeout to prevent hanging on very large UI trees
                let strategy4_start = std::time::Instant::now();
                let timeout_ms = 2000; // 2 second timeout for Desktop search
                
                if let Ok((r4, s4)) = find_by_xpath_detailed(auto, &desktop, &desktop_desc_xpath) {
                    let elapsed = strategy4_start.elapsed().as_millis();
                    if !r4.is_empty() && elapsed < timeout_ms {
                        log::info!("[XPath Fallback] ✓ Strategy 4: Found {} from Desktop root desc ({}ms)", 
                            r4.len(), fallback_start.elapsed().as_millis());
                        return Ok((r4, s4));
                    } else if elapsed >= timeout_ms {
                        log::warn!("[XPath Fallback] Strategy 4 timed out after {}ms, skipping result", elapsed);
                    }
                }
            }
            
            log::info!("[XPath Fallback] All /XPath strategies exhausted ({}ms)", 
                fallback_start.elapsed().as_millis());
            Ok((results, segments))
        }
    }

    /// Parsed representation of a single XPath step (e.g., "Pane[starts-with(@ClassName, 'Chrome_Widget') and @FrameworkId='Win32']")
    struct ParsedXPathStep {
        type_name: Option<String>,
        required_props: Vec<(String, String)>,
        require_starts_with: Vec<(String, String)>,
    }

    /// Parse an XPath step string into its components
    fn parse_xpath_step(step: &str) -> ParsedXPathStep {
        let (type_name, predicates_str): (Option<String>, &str) = if step.starts_with('[') {
            (None, step)
        } else if let Some(bracket_pos) = step.find('[') {
            (Some(step[..bracket_pos].to_string()), &step[bracket_pos..])
        } else {
            (Some(step.to_string()), "")
        };

        let mut required_props: Vec<(String, String)> = Vec::new();
        let mut require_starts_with: Vec<(String, String)> = Vec::new();

        if let Ok(re) = regex::Regex::new(r#"@(\w+)='([^']*)'"#) {
            for cap in re.captures_iter(predicates_str) {
                if let (Some(key), Some(val)) = (cap.get(1), cap.get(2)) {
                    required_props.push((key.as_str().to_string(), val.as_str().to_string()));
                }
            }
        }
        if let Ok(re) = regex::Regex::new(r#"starts-with\(@(\w+),\s*'([^']*)'\)"#) {
            for cap in re.captures_iter(predicates_str) {
                if let (Some(key), Some(val)) = (cap.get(1), cap.get(2)) {
                    require_starts_with.push((key.as_str().to_string(), val.as_str().to_string()));
                }
            }
        }

        ParsedXPathStep { type_name, required_props, require_starts_with }
    }

    /// Check if an IUIAutomationElement matches a parsed XPath step
    fn element_matches_parsed_step(elem: &IUIAutomationElement, step: &ParsedXPathStep) -> bool {
        // Check control type
        if let Some(ref type_name) = step.type_name {
            let elem_ct = unsafe { elem.CurrentControlType().map(control_type_name).unwrap_or_default() };
            if elem_ct != *type_name {
                return false;
            }
        }

        // Check exact property matches
        for (key, val) in &step.required_props {
            let actual = match key.as_str() {
                "Name" => get_bstr(unsafe { elem.CurrentName() }),
                "ClassName" => get_bstr(unsafe { elem.CurrentClassName() }),
                "AutomationId" => get_bstr(unsafe { elem.CurrentAutomationId() }),
                "FrameworkId" => get_bstr(unsafe { elem.CurrentFrameworkId() }),
                _ => String::new(),
            };
            if actual != *val {
                return false;
            }
        }

        // Check starts-with predicates
        for (key, prefix) in &step.require_starts_with {
            let actual = match key.as_str() {
                "ClassName" => get_bstr(unsafe { elem.CurrentClassName() }),
                "Name" => get_bstr(unsafe { elem.CurrentName() }),
                "AutomationId" => get_bstr(unsafe { elem.CurrentAutomationId() }),
                _ => String::new(),
            };
            if !actual.starts_with(prefix) {
                return false;
            }
        }

        true
    }

    /// Walk the raw tree step-by-step to find elements matching the given XPath steps.
    /// Each step in `steps` must be a direct child of the previous match.
    fn walk_raw_tree_steps(
        auto: &IUIAutomation,
        raw_walker: &windows::Win32::UI::Accessibility::IUIAutomationTreeWalker,
        root: &IUIAutomationElement,
        steps: &[&str],
    ) -> anyhow::Result<Vec<IUIAutomationElement>> {
        if steps.is_empty() {
            return Ok(vec![root.clone()]);
        }

        let first_parsed = parse_xpath_step(steps[0]);

        // Find children of root matching the first step
        let mut current_matches: Vec<IUIAutomationElement> = Vec::new();
        let mut child = unsafe { raw_walker.GetFirstChildElement(root).ok() };
        while let Some(c) = child {
            if element_matches_parsed_step(&c, &first_parsed) {
                current_matches.push(c.clone());
            }
            child = unsafe { raw_walker.GetNextSiblingElement(&c).ok() };
        }

        if current_matches.is_empty() {
            return Ok(vec![]);
        }

        // If this is the last step, return the matches
        if steps.len() == 1 {
            return Ok(current_matches);
        }

        // Recurse for remaining steps
        let remaining = &steps[1..];
        let mut all_matches = Vec::new();
        for candidate in &current_matches {
            if let Ok(sub_matches) = walk_raw_tree_steps(auto, raw_walker, candidate, remaining) {
                all_matches.extend(sub_matches);
            }
        }

        Ok(all_matches)
    }

    /// Enumerate all child HWNDs of a given parent HWND using Win32 EnumChildWindows.
    /// This bypasses the UIA tree structure, which may not list child HWND elements
    /// as children of the parent window element.
    fn enum_child_hwnds(parent: HWND) -> Vec<HWND> {
        let hwnds: std::cell::RefCell<Vec<HWND>> = std::cell::RefCell::new(Vec::new());
        
        unsafe extern "system" fn enum_callback(child: HWND, lparam: LPARAM) -> windows::core::BOOL {
            let hwnds = &*(lparam.0 as *const std::cell::RefCell<Vec<HWND>>);
            hwnds.borrow_mut().push(child);
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

    /// Find elements using RawViewWalker to traverse the RAW UIA tree,
    /// then step-by-step match the XPath path. This bypasses ControlViewWalker which
    /// filters out WebView elements (e.g., Chrome_Widget Pane in WeChat).
    ///
    /// KEY INSIGHT: FindAll(TreeScope_Descendants) ALSO uses the control view filter,
    /// so it can't see Chrome_Widget Pane either. Only RawViewWalker sees everything.
    ///
    /// Strategy:
    /// 1. Use RawViewWalker to find the first element matching the first XPath step
    /// 2. Try uiauto-xpath on the remaining XPath from that element
    /// 3. If uiauto-xpath fails (ControlViewWalker can't navigate inside Chrome fragment),
    ///    fall back to walking the raw tree for ALL remaining steps manually
    fn find_by_xpath_raw_descendants(
        auto: &IUIAutomation,
        window: &IUIAutomationElement,
        xpath: &str,
    ) -> anyhow::Result<(Vec<IUIAutomationElement>, Vec<SegmentValidationResult>)> {
        use std::time::Instant;
        let start = Instant::now();

        // Parse XPath steps (skip leading // or /)
        let xpath_parts: Vec<&str> = xpath.split('/').filter(|s| !s.is_empty()).collect();
        if xpath_parts.is_empty() {
            return Ok((vec![], vec![]));
        }

        // Get the first step to match
        let first_step = xpath_parts[0];

        // Parse the first step into (control_type, predicates)
        let first_step_parsed = parse_xpath_step(first_step);
        log::info!("[Raw Desc] First step: type={:?}, exact={:?}, starts_with={:?}",
            first_step_parsed.type_name, first_step_parsed.required_props, first_step_parsed.require_starts_with);

        // Use RawViewWalker to find elements matching the first step
        let raw_walker = match unsafe { auto.RawViewWalker() } {
            Ok(w) => w,
            Err(e) => {
                log::warn!("[Raw Desc] Failed to get RawViewWalker: {}", e);
                return Ok((vec![], vec![]));
            }
        };

        // ── Diagnostic: print raw tree children at depth 1 and 2 ──
        {
            let mut diag_count = 0u32;
            let mut d1_child = unsafe { raw_walker.GetFirstChildElement(window).ok() };
            while let Some(c) = d1_child {
                let ct = unsafe { c.CurrentControlType().map(control_type_name).unwrap_or_default() };
                let cn = get_bstr(unsafe { c.CurrentClassName() });
                let nm = get_bstr(unsafe { c.CurrentName() });
                let fw = get_bstr(unsafe { c.CurrentFrameworkId() });
                let pid = unsafe { c.CurrentProcessId().unwrap_or(0) };
                log::info!("[Raw Desc]   raw_depth1[{}]: {} class='{}' name='{}' fw='{}' pid={}",
                    diag_count, ct, cn, nm, fw, pid);
                // Print depth-2 children of the first 3 depth-1 elements
                if diag_count < 3 {
                    let mut d2_idx = 0u32;
                    let mut d2_child = unsafe { raw_walker.GetFirstChildElement(&c).ok() };
                    while let Some(c2) = d2_child {
                        if d2_idx < 5 {
                            let ct2 = unsafe { c2.CurrentControlType().map(control_type_name).unwrap_or_default() };
                            let cn2 = get_bstr(unsafe { c2.CurrentClassName() });
                            let fw2 = get_bstr(unsafe { c2.CurrentFrameworkId() });
                            log::info!("[Raw Desc]     raw_depth2[{}]: {} class='{}' fw='{}'", d2_idx, ct2, cn2, fw2);
                        }
                        d2_idx += 1;
                        d2_child = unsafe { raw_walker.GetNextSiblingElement(&c2).ok() };
                    }
                    if d2_idx > 5 {
                        log::info!("[Raw Desc]     ... and {} more depth-2 children", d2_idx - 5);
                    }
                }
                diag_count += 1;
                d1_child = unsafe { raw_walker.GetNextSiblingElement(&c).ok() };
            }
            log::info!("[Raw Desc] Window has {} raw children at depth 1", diag_count);
        }

        // Collect raw tree children of the window, then search deeper if needed
        let mut first_step_matches: Vec<IUIAutomationElement> = Vec::new();

        // BFS: search for first-step matches in the raw tree (up to depth 3 to avoid slowness)
        let mut queue: Vec<(IUIAutomationElement, u32)> = vec![(window.clone(), 0)];
        let max_depth = 3u32;

        while let Some((elem, depth)) = queue.pop() {
            let mut child = unsafe { raw_walker.GetFirstChildElement(&elem).ok() };
            while let Some(c) = child {
                // Check if this child matches the first step
                if element_matches_parsed_step(&c, &first_step_parsed) {
                    first_step_matches.push(c.clone());
                }
                // Add to queue for deeper search if within depth limit
                if depth + 1 < max_depth {
                    queue.push((c.clone(), depth + 1));
                }
                child = unsafe { raw_walker.GetNextSiblingElement(&c).ok() };
            }
        }

        log::info!("[Raw Desc] Found {} elements matching first step via RawViewWalker ({}ms)",
            first_step_matches.len(), start.elapsed().as_millis());

        if first_step_matches.is_empty() {
            let duration_ms = start.elapsed().as_millis() as u64;
            return Ok((vec![], vec![SegmentValidationResult {
                segment_index: 0,
                segment_text: first_step.to_string(),
                matched: false,
                match_count: 0,
                duration_ms,
                predicate_failures: vec![super::PredicateFailure {
                    attr_name: "RawTree".to_string(),
                    expected_value: first_step.to_string(),
                    actual_value: None,
                    reason: "No raw tree element matches this step".to_string(),
                }],
            }]));
        }

        // Build remaining XPath (steps after the first)
        let remaining_parts = &xpath_parts[1..];
        let remaining_xpath = if remaining_parts.is_empty() {
            String::new()
        } else {
            format!("/{}", remaining_parts.join("/"))
        };

        // If no remaining steps, the first-step matches ARE the result
        if remaining_parts.is_empty() {
            let duration_ms = start.elapsed().as_millis() as u64;
            let match_count = first_step_matches.len();
            log::info!("[Raw Desc] First step is the last step, returning {} matches ({}ms)",
                match_count, duration_ms);
            return Ok((first_step_matches, vec![SegmentValidationResult {
                segment_index: 0,
                segment_text: xpath.to_string(),
                matched: true,
                match_count,
                duration_ms,
                predicate_failures: Vec::new(),
            }]));
        }

        // Strategy A: Try uiauto-xpath from each first-step match for the remaining XPath
        // This works if ControlViewWalker CAN navigate inside the Chrome fragment
        // (e.g., Document is visible in the control view even though Pane is not)
        for candidate in &first_step_matches {
            if let Ok((matches, segments)) = find_by_xpath_detailed(auto, candidate, &remaining_xpath) {
                if !matches.is_empty() {
                    log::info!("[Raw Desc] ✓ uiauto-xpath found {} from raw candidate ({}ms)",
                        matches.len(), start.elapsed().as_millis());
                    // Prepend a segment for the first step
                    let mut all_segments = vec![SegmentValidationResult {
                        segment_index: 0,
                        segment_text: first_step.to_string(),
                        matched: true,
                        match_count: 1,
                        duration_ms: 0,
                        predicate_failures: Vec::new(),
                    }];
                    for mut s in segments {
                        s.segment_index += 1;
                        all_segments.push(s);
                    }
                    return Ok((matches, all_segments));
                }
            }
        }

        log::info!("[Raw Desc] uiauto-xpath failed from raw candidates, falling back to full raw tree walk ({}ms)",
            start.elapsed().as_millis());

        // Strategy B: Walk the raw tree manually for ALL remaining steps
        // This handles the case where ControlViewWalker can't navigate inside the Chrome fragment
        let mut all_matches = Vec::new();
        for candidate in &first_step_matches {
            if let Ok(matches) = walk_raw_tree_steps(auto, &raw_walker, candidate, remaining_parts) {
                if !matches.is_empty() {
                    all_matches.extend(matches);
                }
            }
        }

        let duration_ms = start.elapsed().as_millis() as u64;
        log::info!("[Raw Desc] Full raw walk found {} matches ({}ms)", all_matches.len(), duration_ms);

        let segments: Vec<SegmentValidationResult> = xpath_parts.iter().enumerate().map(|(i, step)| {
            SegmentValidationResult {
                segment_index: i,
                segment_text: step.to_string(),
                matched: i < xpath_parts.len() - 1 || !all_matches.is_empty(),
                match_count: if i == xpath_parts.len() - 1 { all_matches.len() } else { 0 },
                duration_ms: 0,
                predicate_failures: Vec::new(),
            }
        }).collect();

        Ok((all_matches, segments))
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
    /// When multiple windows match the selector, tries each until one succeeds.
    pub fn find_all_elements_detailed(
        window_selector: &str,
        element_xpath: &str,
        random_range: f32,
    ) -> Vec<crate::api::types::ElementInfo> {
        use crate::api::types::{ElementInfo, Rect, Point};
        use rand::Rng;
        
        let auto = match get_automation() {
            Ok(a) => a,
            Err(_) => return vec![],
        };
        let windows = find_window_by_selector(&auto, window_selector);
        
        if windows.is_empty() {
            return vec![];
        }
        
        // Try each matching window until we find elements
        for window in &windows {
            // 获取窗口矩形用于计算 visibleRect
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
            
            let (elements, _) = match find_by_xpath_with_fallback(&auto, window, element_xpath) {
                Ok(result) => result,
                Err(_) => continue,
            };
            
            if !elements.is_empty() {
                let mut rng = rand::thread_rng();
                
                return elements.iter().filter_map(|elem| {
                    let r = match unsafe { elem.CurrentBoundingRectangle() } {
                        Ok(r) => r,
                        Err(_) => return None,
                    };
                    let api_rect = Rect {
                        x: r.left,
                        y: r.top,
                        width: r.right - r.left,
                        height: r.bottom - r.top,
                    };
                    let center = api_rect.center();
                    
                    // 计算 visibleRect
                    let visible_rect = compute_element_visible_rect(&api_rect, window_rect.as_ref());
                    
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
                    
                    let is_offscreen = unsafe { elem.CurrentIsOffscreen().map(|b| b.as_bool()).unwrap_or(false) };

                    // rect 始终保留：即使元素标记为 offscreen，坐标仍有效（如副屏负坐标场景）
                    // center / center_random 仅在非 offscreen 时提供，避免误点击不可见元素
                    let (center_opt, cr_opt) = if is_offscreen {
                        (None, None)
                    } else {
                        (Some(center), Some(center_random))
                    };

                    Some(ElementInfo {
                        rect: Some(api_rect),
                        visible_rect,
                        center: center_opt,
                        center_random: cr_opt,
                        control_type: unsafe { elem.CurrentControlType().map(control_type_name).unwrap_or_default() },
                        name: get_bstr(unsafe { elem.CurrentName() }),
                        automation_id: get_bstr(unsafe { elem.CurrentAutomationId() }),
                        class_name: get_bstr(unsafe { elem.CurrentClassName() }),
                        framework_id: get_bstr(unsafe { elem.CurrentFrameworkId() }),
                        help_text: get_bstr(unsafe { elem.CurrentHelpText() }),
                        localized_control_type: get_bstr(unsafe { elem.CurrentLocalizedControlType() }),
                        is_enabled: unsafe { elem.CurrentIsEnabled().map(|b| b.as_bool()).unwrap_or(true) },
                        is_offscreen,
                        is_password: unsafe { elem.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(false) },
                        accelerator_key: get_bstr(unsafe { elem.CurrentAcceleratorKey() }),
                        access_key: get_bstr(unsafe { elem.CurrentAccessKey() }),
                        item_type: get_bstr(unsafe { elem.CurrentItemType() }),
                        item_status: get_bstr(unsafe { elem.CurrentItemStatus() }),
                        process_id: unsafe { elem.CurrentProcessId().unwrap_or(0) as u32 },
                        is_checkable: None,
                        is_checked: None,
                        is_clickable: None,
                        is_scrollable: None,
                        is_selected: None,
                    })
                }).collect();
            }
        }
        
        vec![]
    }

    /// Find all matching elements searching from the Desktop root.
    /// Used for batch/common element searches where the target may be in a different
    /// window tree than the window selector (e.g., hybrid Qt+WebView apps like WeChat).
    pub fn find_all_elements_from_root(
        element_xpath: &str,
        random_range: f32,
    ) -> Vec<crate::api::types::ElementInfo> {
        use crate::api::types::{ElementInfo, Rect, Point};
        use rand::Rng;

        let auto = match get_automation() {
            Ok(a) => a,
            Err(_) => return vec![],
        };

        let desktop = match unsafe { auto.GetRootElement() } {
            Ok(d) => d,
            Err(e) => {
                log::error!("[find_from_root] Failed to get root element: {:?}", e);
                return vec![];
            }
        };

        log::info!("[find_from_root] Searching from Desktop root: xpath='{}'", element_xpath);
        let (elements, _) = match find_by_xpath_detailed(&auto, &desktop, element_xpath) {
            Ok(r) => r,
            Err(e) => {
                log::error!("[find_from_root] XPath failed: {}", e);
                return vec![];
            }
        };

        if elements.is_empty() {
            log::info!("[find_from_root] No elements found");
            return vec![];
        }

        log::info!("[find_from_root] Found {} elements", elements.len());

        // 获取 Desktop 矩形用于计算 visibleRect（Desktop 通常覆盖整个屏幕）
        let desktop_rect = unsafe {
            desktop.CurrentBoundingRectangle().ok().map(|r| {
                crate::api::types::Rect {
                    x: r.left,
                    y: r.top,
                    width: r.right - r.left,
                    height: r.bottom - r.top,
                }
            })
        };

        let mut rng = rand::thread_rng();
        elements.iter().filter_map(|elem| {
            let r = match unsafe { elem.CurrentBoundingRectangle() } {
                Ok(r) => r,
                Err(_) => return None,
            };
            let api_rect = Rect {
                x: r.left,
                y: r.top,
                width: r.right - r.left,
                height: r.bottom - r.top,
            };
            let center = api_rect.center();

            // 计算 visibleRect
            let visible_rect = compute_element_visible_rect(&api_rect, desktop_rect.as_ref());

            let half_range_w = api_rect.width as f32 * random_range / 2.0;
            let half_range_h = api_rect.height as f32 * random_range / 2.0;

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

            let is_offscreen = unsafe { elem.CurrentIsOffscreen().map(|b| b.as_bool()).unwrap_or(false) };

            let (rect_opt, center_opt, cr_opt) = if is_offscreen {
                (None, None, None)
            } else {
                (Some(api_rect), Some(center), Some(center_random))
            };

            Some(ElementInfo {
                rect: rect_opt,
                visible_rect,
                center: center_opt,
                center_random: cr_opt,
                control_type: unsafe { elem.CurrentControlType().map(control_type_name).unwrap_or_default() },
                name: get_bstr(unsafe { elem.CurrentName() }),
                automation_id: get_bstr(unsafe { elem.CurrentAutomationId() }),
                class_name: get_bstr(unsafe { elem.CurrentClassName() }),
                framework_id: get_bstr(unsafe { elem.CurrentFrameworkId() }),
                help_text: get_bstr(unsafe { elem.CurrentHelpText() }),
                localized_control_type: get_bstr(unsafe { elem.CurrentLocalizedControlType() }),
                is_enabled: unsafe { elem.CurrentIsEnabled().map(|b| b.as_bool()).unwrap_or(true) },
                is_offscreen,
                is_password: unsafe { elem.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(false) },
                accelerator_key: get_bstr(unsafe { elem.CurrentAcceleratorKey() }),
                access_key: get_bstr(unsafe { elem.CurrentAccessKey() }),
                item_type: get_bstr(unsafe { elem.CurrentItemType() }),
                item_status: get_bstr(unsafe { elem.CurrentItemStatus() }),
                process_id: unsafe { elem.CurrentProcessId().unwrap_or(0) as u32 },
                is_checkable: None,
                is_checked: None,
                is_clickable: None,
                is_scrollable: None,
                is_selected: None,
            })
        }).collect()
    }

    /// 从 UIA 元素提取子元素特征
    /// 
    /// # Arguments
    /// * `automation` - IUIAutomation 实例
    /// * `element` - 目标 UIA 元素
    /// * `parent_rect` - 父元素的矩形（用于计算相对位置）
    /// 
    /// # Returns
    /// 子元素特征列表
    pub fn extract_children_features(
        automation: &IUIAutomation,
        element: &IUIAutomationElement,
        parent_rect: &RECT,
    ) -> Vec<crate::core::model::ChildFeature> {
        use windows::Win32::UI::Accessibility::TreeScope_Children;
        use crate::core::model::{ChildFeature, RelativeRect};
        
        let mut features = vec![];
        
        unsafe {
            // 创建条件：获取所有子元素
            let condition = match automation.CreateTrueCondition() {
                Ok(c) => c,
                Err(_) => return vec![],
            };
            
            // 查找所有直接子元素
            let children_array = match element.FindAll(TreeScope_Children, &condition) {
                Ok(arr) => arr,
                Err(_) => return vec![],
            };
            
            let count = match children_array.Length() {
                Ok(c) => c,
                Err(_) => return vec![],
            };
            
            let parent_width = (parent_rect.right - parent_rect.left) as f32;
            let parent_height = (parent_rect.bottom - parent_rect.top) as f32;
            
            if parent_width <= 0.0 || parent_height <= 0.0 {
                return vec![];
            }
            
            for i in 0..count {
                if let Ok(child) = children_array.GetElement(i) {
                    // 获取子元素的 ControlType ID
                    if let Ok(control_type_id) = child.CurrentControlType() {
                        let control_type = control_type_name(control_type_id);
                        
                        // 获取子元素的边界
                        if let Ok(child_rect) = child.CurrentBoundingRectangle() {
                            let child_width = (child_rect.right - child_rect.left) as f32;
                            let child_height = (child_rect.bottom - child_rect.top) as f32;
                            
                            // 计算相对于父元素的归一化坐标
                            let x_ratio = (child_rect.left - parent_rect.left) as f32 / parent_width;
                            let y_ratio = (child_rect.top - parent_rect.top) as f32 / parent_height;
                            let width_ratio = child_width / parent_width;
                            let height_ratio = child_height / parent_height;
                            
                            // 限制在 [0, 1] 范围内
                            let x_ratio = x_ratio.clamp(0.0, 1.0);
                            let y_ratio = y_ratio.clamp(0.0, 1.0);
                            let width_ratio = width_ratio.clamp(0.0, 1.0);
                            let height_ratio = height_ratio.clamp(0.0, 1.0);
                            
                            features.push(ChildFeature {
                                control_type,
                                relative_bounds: RelativeRect {
                                    x_ratio,
                                    y_ratio,
                                    width_ratio,
                                    height_ratio,
                                },
                            });
                        }
                    }
                }
            }
        }
        
        features
    }

    /// 使用 UIA 原生 PropertyCondition 查询指定容器内可见的元素。
    ///
    /// 相比 XPath 全量查询 + 逐个过滤，此方法在系统层面直接跳过 offscreen 子树，
    /// 避免实例化不可见元素的 COM 对象，性能显著提升（尤其是虚拟化列表场景）。
    ///
    /// # Arguments
    /// * `window_selector` - 窗口选择器，用于定位容器所在窗口
    /// * `container_xpath` - 滚动容器的 XPath（查询范围限定在容器内）
    /// * `control_types` - 要查询的 ControlType 名称列表，如 `["Text"]`、`["Text", "Image"]`
    ///   传空则查询所有可见元素（不按 ControlType 过滤）
    ///
    /// # Returns
    /// 匹配的元素信息列表（仅可见元素，isOffscreen=false）
    pub fn find_visible_elements(
        window_selector: &str,
        container_xpath: &str,
        control_types: &[&str],
    ) -> Vec<crate::api::types::ElementInfo> {
        use crate::api::types::{ElementInfo, Rect};
        use windows::Win32::UI::Accessibility::*;
        use windows::Win32::System::Variant::*;

        let auto = match get_automation() {
            Ok(a) => a,
            Err(_) => return vec![],
        };

        // 1. 定位容器元素
        let windows = find_window_by_selector(&auto, window_selector);
        if windows.is_empty() {
            log::warn!("[find_visible_elements] No window found for selector: {}", window_selector);
            return vec![];
        }

        let mut container_elem: Option<IUIAutomationElement> = None;
        for win in &windows {
            if let Ok((elements, _)) = find_by_xpath_with_fallback(&auto, win, container_xpath) {
                if let Some(first) = elements.first() {
                    container_elem = Some(first.clone());
                    break;
                }
            }
        }

        let container = match container_elem {
            Some(e) => e,
            None => {
                log::warn!("[find_visible_elements] Container not found: {}", container_xpath);
                return vec![];
            }
        };

        // 2. 构建 IsOffscreen = FALSE 条件
        let offscreen_false = {
            let mut variant = VARIANT::default();
            unsafe {
                let var_ptr = &mut variant as *mut VARIANT;
                // VT_BOOL = 11
                let vt_ptr = var_ptr as *mut VARENUM;
                std::ptr::write(vt_ptr, VT_BOOL);
                // VARIANT_BOOL: VARIANT_TRUE = -1 (0xFFFF), VARIANT_FALSE = 0
                // The boolVal field is at offset 8 in the union (same as bstrVal)
                let bool_ptr = (var_ptr as *mut u8).add(8) as *mut i16;
                std::ptr::write(bool_ptr, 0); // VARIANT_FALSE = IsOffscreen = false
            }
            match unsafe { auto.CreatePropertyCondition(UIA_IsOffscreenPropertyId, &variant) } {
                Ok(c) => c,
                Err(e) => {
                    log::error!("[find_visible_elements] CreatePropertyCondition(IsOffscreen=false) failed: {}", e);
                    return vec![];
                }
            }
        };

        // 3. 构建 ControlType 条件（如果指定了 control_types）
        let condition = if control_types.is_empty() {
            // 不按 ControlType 过滤，只用 IsOffscreen=false
            offscreen_false
        } else if control_types.len() == 1 {
            // 单个 ControlType → 直接与 IsOffscreen AND
            let ct_id = match control_type_name_to_id(control_types[0]) {
                Some(id) => id,
                None => {
                    log::warn!("[find_visible_elements] Unknown control type: {}", control_types[0]);
                    return vec![];
                }
            };

            let ct_condition = {
                let mut variant = VARIANT::default();
                unsafe {
                    let var_ptr = &mut variant as *mut VARIANT;
                    let vt_ptr = var_ptr as *mut VARENUM;
                    std::ptr::write(vt_ptr, VT_I4);
                    let i4_ptr = (var_ptr as *mut u8).add(8) as *mut i32;
                    std::ptr::write(i4_ptr, ct_id);
                }
                match unsafe { auto.CreatePropertyCondition(UIA_ControlTypePropertyId, &variant) } {
                    Ok(c) => c,
                    Err(e) => {
                        log::error!("[find_visible_elements] CreatePropertyCondition(ControlType) failed: {}", e);
                        return vec![];
                    }
                }
            };

            match unsafe { auto.CreateAndCondition(&offscreen_false, &ct_condition) } {
                Ok(c) => c,
                Err(e) => {
                    log::error!("[find_visible_elements] CreateAndCondition failed: {}", e);
                    offscreen_false // fallback: 只用 offscreen 条件
                }
            }
        } else {
            // 多个 ControlType → 先用 OrCondition 合并，再与 IsOffscreen AND
            let ct_conditions: Vec<Option<IUIAutomationCondition>> = control_types.iter()
                .filter_map(|ct_name| {
                    let ct_id = control_type_name_to_id(ct_name)?;
                    let mut variant = VARIANT::default();
                    unsafe {
                        let var_ptr = &mut variant as *mut VARIANT;
                        let vt_ptr = var_ptr as *mut VARENUM;
                        std::ptr::write(vt_ptr, VT_I4);
                        let i4_ptr = (var_ptr as *mut u8).add(8) as *mut i32;
                        std::ptr::write(i4_ptr, ct_id);
                    }
                    unsafe { auto.CreatePropertyCondition(UIA_ControlTypePropertyId, &variant) }.ok()
                })
                .map(Some)
                .collect();

            if ct_conditions.is_empty() {
                offscreen_false
            } else if ct_conditions.len() == 1 {
                let ct_cond = ct_conditions[0].clone().unwrap();
                match unsafe { auto.CreateAndCondition(&offscreen_false, &ct_cond) } {
                    Ok(c) => c,
                    Err(_) => offscreen_false,
                }
            } else {
                // OrCondition 合并多个 ControlType
                let ct_or = match unsafe { auto.CreateOrConditionFromNativeArray(&ct_conditions) } {
                    Ok(c) => c,
                    Err(e) => {
                        log::error!("[find_visible_elements] CreateOrCondition failed: {}", e);
                        return vec![];
                    }
                };
                match unsafe { auto.CreateAndCondition(&offscreen_false, &ct_or) } {
                    Ok(c) => c,
                    Err(e) => {
                        log::error!("[find_visible_elements] CreateAndCondition(Offscreen, CT_OR) failed: {}", e);
                        offscreen_false
                    }
                }
            }
        };

        // 4. FindAll(TreeScope_Descendants) 执行查询
        let raw_elements = match unsafe { container.FindAll(TreeScope_Descendants, &condition) } {
            Ok(arr) => arr,
            Err(e) => {
                log::error!("[find_visible_elements] FindAll failed: {}", e);
                return vec![];
            }
        };

        let count = match unsafe { raw_elements.Length() } {
            Ok(c) => c,
            Err(_) => 0,
        };

        log::info!("[find_visible_elements] Found {} visible elements (types={:?})", count, control_types);

        // 获取容器矩形用于计算 visibleRect
        let container_rect = unsafe {
            container.CurrentBoundingRectangle().ok().map(|r| {
                Rect {
                    x: r.left,
                    y: r.top,
                    width: r.right - r.left,
                    height: r.bottom - r.top,
                }
            })
        };

        // 5. 转换为 ElementInfo
        (0..count).filter_map(|i| {
            let elem = match unsafe { raw_elements.GetElement(i) } {
                Ok(e) => e,
                Err(_) => return None,
            };

            let r = match unsafe { elem.CurrentBoundingRectangle() } {
                Ok(r) => r,
                Err(_) => return None,
            };
            let api_rect = Rect {
                x: r.left,
                y: r.top,
                width: r.right - r.left,
                height: r.bottom - r.top,
            };
            let center = api_rect.center();
            
            // 计算 visibleRect
            let visible_rect = compute_element_visible_rect(&api_rect, container_rect.as_ref());
            let is_offscreen = unsafe { elem.CurrentIsOffscreen().map(|b| b.as_bool()).unwrap_or(false) };
            let (center_opt, cr_opt) = if is_offscreen {
                (None, None)
            } else {
                (Some(center), Some(center)) // 不需要随机偏移，center_random = center
            };

            Some(ElementInfo {
                rect: Some(api_rect),
                visible_rect,
                center: center_opt,
                center_random: cr_opt,
                control_type: unsafe { elem.CurrentControlType().map(control_type_name).unwrap_or_default() },
                name: get_bstr(unsafe { elem.CurrentName() }),
                automation_id: get_bstr(unsafe { elem.CurrentAutomationId() }),
                class_name: get_bstr(unsafe { elem.CurrentClassName() }),
                framework_id: get_bstr(unsafe { elem.CurrentFrameworkId() }),
                help_text: get_bstr(unsafe { elem.CurrentHelpText() }),
                localized_control_type: get_bstr(unsafe { elem.CurrentLocalizedControlType() }),
                is_enabled: unsafe { elem.CurrentIsEnabled().map(|b| b.as_bool()).unwrap_or(true) },
                is_offscreen,
                is_password: unsafe { elem.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(false) },
                accelerator_key: get_bstr(unsafe { elem.CurrentAcceleratorKey() }),
                access_key: get_bstr(unsafe { elem.CurrentAccessKey() }),
                item_type: get_bstr(unsafe { elem.CurrentItemType() }),
                item_status: get_bstr(unsafe { elem.CurrentItemStatus() }),
                process_id: unsafe { elem.CurrentProcessId().unwrap_or(0) as u32 },
                is_checkable: None,
                is_checked: None,
                is_clickable: None,
                is_scrollable: None,
                is_selected: None,
            })
        }).collect()
    }
}

// ─── Public API ──────────────────────────────────────────────────────────────

pub use windows_impl::*;

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