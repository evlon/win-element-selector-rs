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
    use windows::core::Interface;
    use windows::{
        core::BSTR,
        Win32::{
            Foundation::{POINT, HWND, LPARAM, RECT},
            System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
            UI::{
                Accessibility::{
                    CUIAutomation, IUIAutomation, IUIAutomationElement,
                    IUIAutomationTreeWalker,
                },
                WindowsAndMessaging::{
                    GetCursorPos, EnumChildWindows, EnumWindows, GetWindowThreadProcessId,
                    IsWindowVisible,
                },
            },
        },
    };

    /// Well-known WebView/browser container class name prefixes.
    /// Shared by `find_content_root` and the Strategy 2.5 heuristic skip.
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

    /// Check if a class name starts with any known WebView prefix.
    fn is_webview_class(class_name: &str) -> bool {
        WEBVIEW_CLASS_PREFIXES.iter().any(|prefix| class_name.starts_with(prefix))
    }

    /// Check if a screen point is within a bounding rectangle (inclusive of edges).
    #[inline]
    fn point_in_rect(x: i32, y: i32, r: &RECT) -> bool {
        x >= r.left && x <= r.right && y >= r.top && y <= r.bottom
    }

    /// Candidate element metadata for the "pick innermost" selection logic.
    /// Extracted as a pure data struct so the selection algorithm can be tested
    /// without COM / IUIAutomationElement dependencies.
    #[derive(Debug, Clone)]
    #[allow(dead_code)] // fields used in tests and for diagnostic logging
    struct CandidateElement {
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

    /// Control types that represent leaf/end-user elements (not containers).
    /// Leaf elements are typically what the user wants to target — they carry
    /// visible content (text, buttons, etc.) rather than structural grouping.
    fn is_leaf_control_type(ct: &str) -> bool {
        matches!(ct,
            "Text" | "Button" | "Hyperlink" | "Edit" | "CheckBox" | "RadioButton"
            | "ComboBox" | "ListItem" | "TreeItem" | "TabItem" | "MenuItem"
            | "DataItem" | "Image" | "ScrollBar" | "Slider" | "Spinner"
            | "ProgressBar" | "Thumb"
        )
    }

    /// Decide whether `candidate` dominates (should replace) the current best.
    /// 
    /// Selection priority (Step 3 / Step 3.1 — FindAll filtering):
    /// 1. **Leaf preference** — a leaf element with a meaningful name dominates
    ///    a container element with an empty name. This fixes the case where
    ///    Chrome's UIA tree has tiny Group wrappers/overlays that beat meaningful
    ///    Text elements due to having a smaller area. A Group with empty name
    ///    is useless for XPath targeting; a Text with a name is always more useful.
    /// 2. **Smallest area** — the most specific / deepest leaf element
    /// 3. **Later array index** (tiebreaker) — deeper in tree traversal order
    ///
    /// NOTE: We deliberately do NOT use RuntimeId length as a depth proxy.
    /// Chrome's UIA provider assigns varying-length RuntimeIds regardless of
    /// tree depth, causing intermediate Group elements to be incorrectly selected
    /// over deeper Text elements. See the unit tests for concrete examples.
    #[allow(dead_code)] // Used in tests; kept for future BFS sibling selection logic
    fn candidate_dominates_findall(
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

    /// Decide whether a drill-down child dominates (should replace) the current deeper candidate.
    /// 
    /// Selection priority (Step 3.5 — RawViewWalker drill-down):
    /// 1. **Smaller area** — strictly smaller always wins
    /// 2. **Equal area with longer RuntimeId** — for equal-area containers (common in Qt/Chrome
    ///    where Group/Pane shares the same rect as its parent), longer RuntimeId hints at a
    ///    deeper element. This is acceptable here because drill-down walks the Raw View tree
    ///    directly (not FindAll), so RuntimeId length is more meaningful.
    #[allow(dead_code)] // Used in tests; kept for potential future drill-down scenarios
    fn candidate_dominates_drilldown(child_area: i64, child_rid_len: usize, deeper_area: i64, deeper_rid_len: usize) -> bool {
        child_area < deeper_area
            || (child_area == deeper_area && child_rid_len > deeper_rid_len)
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

    /// 从 IUIAutomationElement 构建 ElementInfo
    ///
    /// 统一的元素信息构造函数，供 find_all_elements_detailed 和 find_all_elements_from_root 共用。
    /// 统一 isOffscreen 处理：rect 始终保留（即使元素标记为 offscreen，坐标仍有效），
    /// center / center_random 仅在非 offscreen 时提供。
    fn element_info_from_uia<R: rand::Rng>(
        elem: &IUIAutomationElement,
        container_rect: Option<&crate::api::types::Rect>,
        random_range: f32,
        rng: &mut R,
    ) -> Option<crate::api::types::ElementInfo> {
        use crate::api::types::{ElementInfo, Rect, Point};

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

        // 2. Build ancestor chain using RawViewWalker
        // Always use RawViewWalker (not FindAll(Ancestors)) because FindAll uses
        // Control View which filters out intermediate elements (e.g. Qt Group nodes),
        // causing hierarchy gaps (depth 7 jumping to depth 13).
        let walker = unsafe { auto.RawViewWalker() }
            .or_else(|_| unsafe { auto.ControlViewWalker() })?;
        let desktop = unsafe { auto.GetRootElement()? };

        let mut chain: Vec<IUIAutomationElement> = vec![target.clone()];
        let mut current = unsafe { walker.GetParentElement(&target).ok() };
        while let Some(elem) = current {
            let is_desktop = unsafe { auto.CompareElements(&elem, &desktop).unwrap_or(windows::core::BOOL(0)).as_bool() };
            chain.push(elem.clone());
            if is_desktop {
                break;
            }
            current = unsafe { walker.GetParentElement(&elem).ok() };
        }
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
            let walker = unsafe { auto.RawViewWalker().ok() }
                .or_else(|| unsafe { auto.ControlViewWalker().ok() });
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
                if let Ok(toggle) = pattern.cast::<IToggleProvider>() {
                    if let Ok(state) = unsafe { toggle.ToggleState() } {
                        // ToggleState_Off = 0, ToggleState_On = 1, ToggleState_Indeterminate = 2
                        node.is_checked = Some(state.0 == 1);
                    }
                }
            }
        }

        // Read SelectionItem IsSelected if available
        if has_selection_item {
            if let Ok(pattern) = unsafe {
                elem.GetCurrentPattern(UIA_SelectionItemPatternId)
            } {
                use windows::Win32::UI::Accessibility::ISelectionItemProvider;
                if let Ok(sel) = pattern.cast::<ISelectionItemProvider>() {
                    if let Ok(selected) = unsafe { sel.IsSelected() } {
                        node.is_selected = Some(selected.as_bool());
                    }
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
                log::info!("[XPath Validation] Window's direct children (RawViewWalker):");
                let walker = unsafe { auto.RawViewWalker().ok() }
                    .or_else(|| unsafe { auto.ControlViewWalker().ok() });
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

        // Step 1: ElementFromPoint to get the top-level element at cursor
        let mut hit_elem = unsafe {
            auto.ElementFromPoint(pt)
                .map_err(|e| anyhow::anyhow!("ElementFromPoint: {}", e))?
        };
        #[allow(unused_assignments)] // hit_name/_hit_ct updated in Steps 1.1/1.5 for diagnostics
        let mut hit_name = get_bstr(unsafe { hit_elem.CurrentName() });
        let mut _hit_ct = unsafe { hit_elem.CurrentControlType().map(control_type_name).unwrap_or_default() };
        let mut hit_fwid = get_bstr(unsafe { hit_elem.CurrentFrameworkId() });
        debug!("[Enhanced] ElementFromPoint: type='{}' name='{}' fwid='{}'", _hit_ct, hit_name, hit_fwid);

        let my_pid = std::process::id();

        // Step 1.1: If ElementFromPoint hit our own process (highlight/overlay window),
        // try to find the real target element beneath it by walking the Desktop children
        // and finding the deepest non-self element whose BoundingRectangle contains the point.
        if let Ok(pid) = unsafe { hit_elem.CurrentProcessId() } {
            if pid as u32 == my_pid {
                info!("[Enhanced] ElementFromPoint hit own process (type='{}' name='{}'), searching for real target",
                    _hit_ct, hit_name);
                let desktop = unsafe { auto.GetRootElement()? };
                let walker = match unsafe { auto.RawViewWalker().ok() }
                    .or_else(|| unsafe { auto.ControlViewWalker().ok() }) {
                    Some(w) => w,
                    None => { info!("[Enhanced] No walker available, skipping self-process fix"); return Ok(CaptureResult { hierarchy: vec![], cursor_x: x, cursor_y: y, error: Some("无法获取 TreeWalker".into()), window_info: None }); }
                };
                // Enumerate Desktop children to find the topmost non-self window containing the point
                let mut child = unsafe { walker.GetFirstChildElement(&desktop).ok() };
                while let Some(c) = child {
                    if let Ok(c_pid) = unsafe { c.CurrentProcessId() } {
                        if c_pid as u32 == my_pid {
                            child = unsafe { walker.GetNextSiblingElement(&c).ok() };
                            continue;
                        }
                    }
                    if let Ok(c_rect) = unsafe { c.CurrentBoundingRectangle() } {
                        let cw = c_rect.right - c_rect.left;
                        let ch = c_rect.bottom - c_rect.top;
                        if cw > 0 && ch > 0 && point_in_rect(x, y, &c_rect) {
                            // Found a non-self window containing the point; use it
                            // Try to get a deeper element at the same point via FindAll
                            let c_name = get_bstr(unsafe { c.CurrentName() });
                            let c_ct = unsafe { c.CurrentControlType().map(control_type_name).unwrap_or_default() };
                            let c_fwid = get_bstr(unsafe { c.CurrentFrameworkId() });
                            info!("[Enhanced] Found real target window: type='{}' name='{}' fwid='{}'",
                                c_ct, c_name, c_fwid);
                            hit_elem = c;
                            hit_name = c_name;
                            _hit_ct = c_ct;
                            hit_fwid = c_fwid;
                            break;
                        }
                    }
                    child = unsafe { walker.GetNextSiblingElement(&c).ok() };
                }
            }
        }

        // PID of the (possibly Step-1.1-corrected) hit element.
        // Used by Step 2.5 to enumerate same-process windows when BFS fails.
        let hit_pid = unsafe { hit_elem.CurrentProcessId() }
            .map(|p| p as u32)
            .unwrap_or(0);

        // Save the original hit element for fallback (normal capture chain).
        // Must be after Step 1.1 so we save the corrected element (not our own overlay).
        let original_hit_elem = hit_elem.clone();

        // Save pre-Step-1.5 element for FindAll retry.
        // Step 1.5 may replace hit_elem with a cross-HWND element whose BoundingRectangle
        // covers the cursor but whose subtree has no matching elements at that point
        // (e.g., WeChat's Chrome HWND covers the entire window but cursor is in Qt area).
        // In that case, we retry FindAll with the pre-Step-1.5 element.
        let pre_cross_hwnd_elem = hit_elem.clone();
        let _pre_cross_hwnd_ct = _hit_ct.clone();
        let _pre_cross_hwnd_name = hit_name.clone();

        // Step 1.5: Cross-HWND probe
        // In apps like WeChat, ElementFromPoint returns a Qt element, but the real
        // target is inside a Chrome/WebView child HWND. The Chrome element is NOT a
        // UIA descendant of the Qt element, so FindAll(Subtree) and RawViewWalker
        // drill-down cannot reach it. We enumerate child HWNDs of the window,
        // find ones from a different framework that contain the cursor point, and
        // use their root element as the starting point instead.
        let original_hit_fwid = hit_fwid.clone();

        // Find the window HWND by walking up from hit_elem
        let window_hwnd = (|| -> Option<HWND> {
            // Try the hit element itself first
            if let Ok(h) = unsafe { hit_elem.CurrentNativeWindowHandle() } {
                return Some(HWND(h.0));
            }
            // Walk up ancestors to find one with a window handle
            let walker = unsafe { auto.RawViewWalker().ok() }
                .or_else(|| unsafe { auto.ControlViewWalker().ok() })?;
            let mut cur = unsafe { walker.GetParentElement(&hit_elem).ok() };
            while let Some(ancestor) = cur {
                if let Ok(h) = unsafe { ancestor.CurrentNativeWindowHandle() } {
                    return Some(HWND(h.0));
                }
                cur = unsafe { walker.GetParentElement(&ancestor).ok() };
            }
            None
        })();

        if let Some(hwnd) = window_hwnd {
            let child_hwnds = enum_child_hwnds(hwnd);
            if !child_hwnds.is_empty() {
                debug!("[Enhanced] Found {} child HWNDs, probing for cross-framework element", child_hwnds.len());
                for child_hwnd in &child_hwnds {
                    if let Ok(child_elem) = unsafe { auto.ElementFromHandle(*child_hwnd) } {
                        let c_fwid = get_bstr(unsafe { child_elem.CurrentFrameworkId() });
                        let c_ct_local = unsafe { child_elem.CurrentControlType().map(control_type_name).unwrap_or_default() };
                        // Skip same-framework children (Qt → Qt is not a cross-framework transition)
                        if c_fwid == original_hit_fwid { continue; }
                        // Check if this child element contains the cursor point
                        if let Ok(rect) = unsafe { child_elem.CurrentBoundingRectangle() } {
                            let w = rect.right - rect.left;
                            let h = rect.bottom - rect.top;
                            if w > 0 && h > 0 && point_in_rect(x, y, &rect) {
                                let c_name = get_bstr(unsafe { child_elem.CurrentName() });
                                debug!("[Enhanced] Cross-HWND: child type='{}' name='{}' fwid='{}' contains point — using as hit element",
                                    c_ct_local, c_name, c_fwid);
                                hit_elem = child_elem;
                                _hit_ct = c_ct_local;
                                hit_name = c_name; // tracked for diagnostics
                                break;
                            }
                        }
                    }
                }
            }
        } else {
            debug!("[Enhanced] Step 1.5: no window handle found, skipping cross-HWND probe");
        }

        // Step 2: BFS-style traversal from hit_elem to find the deepest element at the cursor point.
        //
        // Instead of FindAll (which returns a flat list of ALL descendants and requires
        // area-based heuristics to estimate depth), we walk the RawViewWalker tree
        // level by level. At each level, we scan the direct children and pick the one
        // whose BoundingRectangle contains the cursor point. Then we descend into that
        // child and repeat, until no child contains the point.
        //
        // This naturally encodes depth — each BFS level IS one tree level — so we don't
        // need area as a depth proxy. When multiple siblings contain the point, we apply
        // leaf preference (leaf with name > container without name) then smallest area.
        //
        // Compared to the old FindAll + drill-down approach:
        // - FindAll(Subtree) returns a flat array; depth is unknown → area heuristic
        //   needed → tiny Group overlays beat large Text elements (BUG)
        // - Drill-down started from the (possibly wrong) FindAll result → too late
        // - BFS starts from the top and walks down → depth is inherent in the traversal
        let cross_hwnd_changed = !unsafe { auto.CompareElements(&hit_elem, &pre_cross_hwnd_elem)
            .unwrap_or(windows::core::BOOL(0)).as_bool() };

        let raw_walker = unsafe { auto.RawViewWalker() }
            .or_else(|_| unsafe { auto.ControlViewWalker() })
            .map_err(|e| anyhow::anyhow!("RawViewWalker: {}", e))?;

        /// Inner BFS loop: walk the tree from `start_elem` level by level,
        /// returning the deepest element whose BoundingRectangle contains (x, y).
        fn bfs_find_deepest(
            walker: &IUIAutomationTreeWalker,
            start_elem: &IUIAutomationElement,
            x: i32, y: i32,
            my_pid: u32,
        ) -> Option<IUIAutomationElement> {
            let mut current = start_elem.clone();
            let mut visited_rids: HashSet<Vec<i32>> = HashSet::new();
            if let Some(rid) = runtime_id_key(&current) { visited_rids.insert(rid); }
            let mut depth: u32 = 0;

            loop {
                if depth > 30 { break; } // Safety: prevent infinite loop

                let mut best_child: Option<IUIAutomationElement> = None;
                let mut best_area = i64::MAX;
                let mut best_ct = String::new();
                let mut best_name = String::new();
                let mut best_is_leaf_with_name = false;

                let mut child = unsafe { walker.GetFirstChildElement(&current).ok() };
                while let Some(c) = child {
                    // Skip elements from our own process (highlight/overlay windows)
                    if let Ok(pid) = unsafe { c.CurrentProcessId() } {
                        if pid as u32 == my_pid {
                            child = unsafe { walker.GetNextSiblingElement(&c).ok() };
                            continue;
                        }
                    }
                    // Skip offscreen elements
                    if let Ok(offscreen) = unsafe { c.CurrentIsOffscreen() } {
                        if offscreen.0 != 0 {
                            child = unsafe { walker.GetNextSiblingElement(&c).ok() };
                            continue;
                        }
                    }
                    // Cycle detection
                    if let Some(rid) = runtime_id_key(&c) {
                        if visited_rids.contains(&rid) {
                            child = unsafe { walker.GetNextSiblingElement(&c).ok() };
                            continue;
                        }
                    }
                    // Check BoundingRectangle contains the point
                    let rect = match unsafe { c.CurrentBoundingRectangle() } {
                        Ok(r) => r,
                        Err(_) => { child = unsafe { walker.GetNextSiblingElement(&c).ok() }; continue; }
                    };
                    let w = rect.right - rect.left;
                    let h = rect.bottom - rect.top;
                    if w <= 0 || h <= 0 || !point_in_rect(x, y, &rect) {
                        child = unsafe { walker.GetNextSiblingElement(&c).ok() };
                        continue;
                    }

                    let area = w as i64 * h as i64;
                    let ct = unsafe { c.CurrentControlType().map(control_type_name).unwrap_or_default() };
                    let name = get_bstr(unsafe { c.CurrentName() });
                    let c_is_leaf_with_name = is_leaf_control_type(&ct) && !name.is_empty();

                    debug!("[BFS]   depth={} match: type='{}' name='{}' area={} leaf_with_name={}",
                        depth, ct, name, area, c_is_leaf_with_name);

                    // Selection among siblings at this level:
                    // 1. Leaf with name beats container without name (leaf preference)
                    //    — fixes tiny Group overlays that beat larger Text elements
                    // 2. Smallest area (most specific element)
                    //    — for equal-area containers (Qt/Chrome intermediate Group/Pane),
                    //      pick any; the next BFS level will refine further
                    let should_replace = if c_is_leaf_with_name && !best_is_leaf_with_name {
                        true  // Leaf preference: leaf with name beats container without name
                    } else if !c_is_leaf_with_name && best_is_leaf_with_name {
                        false // Don't replace leaf with container
                    } else {
                        area < best_area
                    };

                    if should_replace {
                        best_child = Some(c.clone());
                        best_area = area;
                        best_ct = ct;
                        best_name = name;
                        best_is_leaf_with_name = c_is_leaf_with_name;
                    }

                    child = unsafe { walker.GetNextSiblingElement(&c).ok() };
                }

                match best_child {
                    Some(c) => {
                        if let Some(rid) = runtime_id_key(&c) { visited_rids.insert(rid); }
                        debug!("[BFS] depth={} → type='{}' name='{}' area={}",
                            depth, best_ct, best_name, best_area);
                        current = c;
                        depth += 1;
                    }
                    None => break, // No child contains the point → current is deepest
                }
            }

            if depth > 0 { Some(current) } else { None }
        }

        // Try BFS from the current hit_elem (which may have been changed by Step 1.5)
        let mut target_elem = bfs_find_deepest(&raw_walker, &hit_elem, x, y, my_pid);

        // Step 2.1: Retry with pre-Step-1.5 element if BFS found nothing and
        // Step 1.5 changed hit_elem (cross-HWND scenario).
        // This handles the case where WeChat's Chrome HWND covers the entire window
        // but the cursor is in the Qt area — the Chrome element's subtree has no
        // matching elements, so we retry with the Qt element.
        if target_elem.is_none() && cross_hwnd_changed {
            info!("[Enhanced] BFS from cross-HWND element found no matches, retrying with pre-Step-1.5 element");
            target_elem = bfs_find_deepest(&raw_walker, &pre_cross_hwnd_elem, x, y, my_pid);
        }

        // Step 2.5: Same-process window enumeration.
        // If BFS found nothing, the target element might be in a different HWND tree
        // within the same process (e.g., a multi-window app where ElementFromPoint
        // returned a top-level container, but the real element is in a child HWND
        // of a different top-level window of the same process).
        // We enumerate all top-level windows belonging to hit_elem's PID,
        // then their child HWNDs, to find the most specific HWND containing (x,y).
        if target_elem.is_none() && hit_pid != 0 && hit_pid != my_pid {
            info!("[Enhanced] Step 2.5: searching same-process windows (PID={})", hit_pid);
            let top_hwnds = enumerate_top_level_windows();
            // Filter to windows belonging to the same PID
            let same_pid_hwnds: Vec<HWND> = top_hwnds.into_iter().filter(|hwnd| {
                let mut pid: u32 = 0;
                unsafe { GetWindowThreadProcessId(*hwnd, Some(&mut pid)) };
                pid == hit_pid
            }).collect();
            debug!("[Enhanced] Step 2.5: found {} top-level HWNDs for PID={}", same_pid_hwnds.len(), hit_pid);

            for top_hwnd in &same_pid_hwnds {
                let children = enum_child_hwnds(*top_hwnd);
                for child_hwnd in &children {
                    if let Ok(child_elem) = unsafe { auto.ElementFromHandle(*child_hwnd) } {
                        if let Ok(rect) = unsafe { child_elem.CurrentBoundingRectangle() } {
                            let w = rect.right - rect.left;
                            let h = rect.bottom - rect.top;
                            if w > 0 && h > 0 && point_in_rect(x, y, &rect) {
                                let c_ct = unsafe { child_elem.CurrentControlType().map(control_type_name).unwrap_or_default() };
                                let c_name = get_bstr(unsafe { child_elem.CurrentName() });
                                debug!("[Enhanced] Step 2.5: child HWND type='{}' name='{}' contains point — trying BFS",
                                    c_ct, c_name);
                                target_elem = bfs_find_deepest(&raw_walker, &child_elem, x, y, my_pid);
                                if target_elem.is_some() {
                                    info!("[Enhanced] Step 2.5: found element via same-process child HWND");
                                    break;
                                }
                            }
                        }
                    }
                }
                if target_elem.is_some() { break; }
            }
        }

        // If no matching element found after all retries, fall back to original_hit_elem
        // (the ElementFromPoint result) — equivalent to normal capture, which is always reliable.
        let mut target_elem = match target_elem {
            Some(e) => {
                debug!("[Enhanced] SELECTED via BFS: type='{}' name='{}'",
                    unsafe { e.CurrentControlType().map(control_type_name).unwrap_or_default() },
                    get_bstr(unsafe { e.CurrentName() }));
                e
            }
            None => {
                info!("[Enhanced] BFS found no matches, falling back to ElementFromPoint result");
                hit_elem = original_hit_elem.clone();
                let desktop = unsafe { auto.GetRootElement()? };
                let mut ch: Vec<IUIAutomationElement> = vec![hit_elem.clone()];
                let mut cur = unsafe { raw_walker.GetParentElement(&hit_elem).ok() };
                while let Some(elem) = cur {
                    let is_desktop = unsafe { auto.CompareElements(&elem, &desktop).unwrap_or(windows::core::BOOL(0)).as_bool() };
                    ch.push(elem.clone());
                    if is_desktop { break; }
                    cur = unsafe { raw_walker.GetParentElement(&elem).ok() };
                }
                ch.reverse();
                let window_index = 1;
                let mut hierarchy: Vec<HierarchyNode> = Vec::with_capacity(ch.len());
                for (chain_idx, elem) in ch.iter().enumerate() {
                    if let Some(mut node) = element_to_node(elem, &auto) {
                        node.depth_from_window = chain_idx.saturating_sub(window_index);
                        hierarchy.push(node);
                    }
                }
                if let Some(last) = hierarchy.last_mut() {
                    last.is_target = true;
                    last.index = sibling_index(&hit_elem, &raw_walker).unwrap_or(0);
                    if last.index > 0 {
                        if let Some(f) = last.filters.iter_mut().find(|f| f.name == "Index") {
                            f.value = last.index.to_string(); f.enabled = true;
                        }
                        last.sibling_count = count_siblings(&hit_elem, &raw_walker).unwrap_or(0);
                    }
                }
                let window_info = extract_window_info(&hierarchy);
                info!("[Enhanced] Fallback hierarchy depth={}", hierarchy.len());
                return Ok(CaptureResult { hierarchy, cursor_x: x, cursor_y: y, error: None, window_info });
            }
        };

        // Step 4: Build ancestor chain using RawViewWalker (same walker from drill-down)
        // Always use RawViewWalker (not FindAll(Ancestors)) because FindAll uses
        // Control View which filters out intermediate elements (e.g. Qt Group nodes).
        // This ensures the ancestor chain is complete and matches the XPath search side.
        let desktop = unsafe { auto.GetRootElement()? };

        let build_chain = |target: &IUIAutomationElement| -> Vec<IUIAutomationElement> {
            let mut ch: Vec<IUIAutomationElement> = vec![target.clone()];
            let mut cur = unsafe { raw_walker.GetParentElement(target).ok() };
            while let Some(elem) = cur {
                let is_desktop = unsafe { auto.CompareElements(&elem, &desktop).unwrap_or(windows::core::BOOL(0)).as_bool() };
                ch.push(elem.clone());
                if is_desktop {
                    break;
                }
                cur = unsafe { raw_walker.GetParentElement(&elem).ok() };
            }
            ch.reverse();
            ch
        };

        let mut chain = build_chain(&target_elem);
        debug!("[Enhanced] RawViewWalker chain length = {}", chain.len());

        // Step 4.5: Log chain info for diagnostics.
        // Previous PID-based validation was too strict and caused false fallbacks.
        // The PID filtering in FindAll (Step 3) and drill-down (Step 3.5) already
        // prevents selecting elements from our own process. If the chain is
        // suspiciously short (< 3 = Desktop + Window + target), fall back.
        if chain.len() < 3 {
            info!("[Enhanced] Chain too short (len={}), falling back to normal capture chain", chain.len());
            chain = build_chain(&original_hit_elem);
            target_elem = original_hit_elem.clone();
            info!("[Enhanced] Fallback chain length = {}", chain.len());
        } else {
            // Log chain[1] (the window element) for diagnostics
            if let Some(win_elem) = chain.get(1) {
                let win_ct = unsafe { win_elem.CurrentControlType().map(control_type_name).unwrap_or_default() };
                let win_name = get_bstr(unsafe { win_elem.CurrentName() });
                let win_pid = unsafe { win_elem.CurrentProcessId().unwrap_or(0) };
                info!("[Enhanced] Chain OK (len={}): window type='{}' name='{}' pid={}",
                    chain.len(), win_ct, win_name, win_pid);
            }
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

        if let Some(last) = hierarchy.last_mut() {
            last.is_target = true;
            last.index = sibling_index(&target_elem, &raw_walker).unwrap_or(0);
            if last.index > 0 {
                if let Some(f) = last.filters.iter_mut().find(|f| f.name == "Index") {
                    f.value = last.index.to_string(); f.enabled = true;
                }
                last.sibling_count = count_siblings(&target_elem, &raw_walker).unwrap_or(0);
            }
        }

        let window_info = extract_window_info(&hierarchy);
        let empty = String::new();
        let empty2 = String::new();
        let target_ct = hierarchy.last().map(|n| &n.control_type).unwrap_or(&empty);
        let target_name = hierarchy.last().map(|n| &n.name).unwrap_or(&empty2);
        let normal_ct = unsafe { original_hit_elem.CurrentControlType().map(control_type_name).unwrap_or_default() };
        let normal_name = get_bstr(unsafe { original_hit_elem.CurrentName() });
        info!("[Enhanced] hierarchy depth={} target='{}' name='{}' | ElementFromPoint type='{}' name='{}'",
            hierarchy.len(), target_ct, target_name, normal_ct, normal_name);

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
        let walker = unsafe { auto.RawViewWalker().ok() }
            .or_else(|| unsafe { auto.ControlViewWalker().ok() })?;
        let window_fwid = get_bstr(unsafe { window.CurrentFrameworkId() });
        
        log::info!("[Content Root] Window FrameworkId='{}', scanning children...", window_fwid);
        
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
            
            // Step 3: EnumChildWindows + raw tree walk.
            // For apps like WeChat, the Chrome child HWND is not visible as a UIA
            // descendant of the window, so //Document[…] searches from window root
            // will miss it.  find_by_xpath_detailed (uiauto-xpath) also fails because
            // it uses the control view tree.  We try both approaches:
            //   3a. uiauto-xpath from each child HWND (works if control view can navigate)
            //   3b. find_by_xpath_raw_descendants from each child HWND (raw tree BFS + walk)
            if let Ok(hwnd) = unsafe { window.CurrentNativeWindowHandle() } {
                let child_hwnds = enum_child_hwnds(HWND(hwnd.0));
                log::info!("[XPath Fallback] //XPath — Step 3: trying {} child HWNDs", child_hwnds.len());
                for (idx, child_hwnd) in child_hwnds.iter().enumerate() {
                    if let Ok(child_elem) = unsafe { auto.ElementFromHandle(*child_hwnd) } {
                        // 3a: uiauto-xpath from child HWND
                        if let Ok((r, s)) = find_by_xpath_detailed(auto, &child_elem, xpath) {
                            if !r.is_empty() {
                                log::info!("[XPath Fallback] ✓ Step 3a: Found {} from child HWND[{}] via uiauto-xpath ({}ms)",
                                    r.len(), idx, fallback_start.elapsed().as_millis());
                                return Ok((r, s));
                            }
                        }
                        // 3b: raw tree descendants from child HWND (critical for Chrome/WebView)
                        if let Ok((r, s)) = find_by_xpath_raw_descendants(auto, &child_elem, xpath) {
                            if !r.is_empty() {
                                log::info!("[XPath Fallback] ✓ Step 3b: Found {} from child HWND[{}] via raw walk ({}ms)",
                                    r.len(), idx, fallback_start.elapsed().as_millis());
                                return Ok((r, s));
                            }
                        }
                    }
                }
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
            
            // Strategy 1.5: RawViewWalker BFS from window root.
            // ControlViewWalker (used by uiauto-xpath) cannot see elements filtered from
            // the control view, such as Chrome_Widget Pane in WeChat. Use RawViewWalker
            // to walk the raw tree, find elements matching the first XPath step, then
            // try the remaining path from each match.
            {
                let xpath_parts: Vec<&str> = xpath.split('/').filter(|s| !s.is_empty()).collect();
                if !xpath_parts.is_empty() {
                    log::info!("[XPath Fallback] /XPath — Strategy 1.5: RawViewWalker BFS from window root");
                    if let Ok(raw_walker) = unsafe { auto.RawViewWalker() } {
                        let first_parsed = parse_xpath_step(xpath_parts[0]);
                        let first_step_end = find_first_step_end(xpath);
                        let remaining_after_first = &xpath[first_step_end..];
                        
                        // BFS from window root using RawViewWalker, max depth 8
                        let mut queue: Vec<(IUIAutomationElement, u32)> = vec![(window.clone(), 0)];
                        let mut visited: HashSet<Vec<i32>> = HashSet::new();
                        if let Some(rid) = runtime_id_key(window) { visited.insert(rid); }
                        
                        while let Some((elem, depth)) = queue.pop() {
                            if depth > 8 { continue; }
                            
                            // Check this element's children
                            let mut child = unsafe { raw_walker.GetFirstChildElement(&elem).ok() };
                            while let Some(c) = child {
                                if let Some(rid) = runtime_id_key(&c) {
                                    if !visited.insert(rid) {
                                        child = unsafe { raw_walker.GetNextSiblingElement(&c).ok() };
                                        continue;
                                    }
                                }
                                
                                if element_matches_parsed_step(&c, &first_parsed) {
                                    log::info!("[XPath Fallback] Strategy 1.5: found first-step match at depth {}", depth + 1);
                                    if let Some(result) = try_remaining_from_match(
                                        auto, &c, remaining_after_first, &xpath_parts,
                                        &fallback_start, "1.5",
                                    ) {
                                        return Ok(result);
                                    }
                                }
                                
                                // Also enqueue for deeper BFS
                                queue.push((c.clone(), depth + 1));
                                child = unsafe { raw_walker.GetNextSiblingElement(&c).ok() };
                            }
                        }
                        log::info!("[XPath Fallback] Strategy 1.5: no match found via RawViewWalker BFS");
                    }
                }
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
            //
            // HEURISTIC SKIP: If the first XPath step references a WebView class (e.g., Chrome_Widget),
            // WebView elements are NOT direct raw-tree children of the window — they live under a
            // child HWND. Strategy 2.5's BFS would waste ~200ms finding nothing, so skip to 2.7.
            {
                let xpath_parts: Vec<&str> = xpath.split('/').filter(|s| !s.is_empty()).collect();
                let first_step_has_webview = xpath_parts.first().map_or(false, |step| {
                    let parsed = parse_xpath_step(step);
                    parsed.require_starts_with.iter().any(|(k, v)| {
                        k == "ClassName" && is_webview_class(v)
                    }) || parsed.required_props.iter().any(|(k, v)| {
                        k == "ClassName" && is_webview_class(v)
                    })
                });
                
                if first_step_has_webview {
                    log::info!("[XPath Fallback] /XPath — Skipping Strategy 2.5: first step has WebView class, going directly to 2.7");
                } else {
                    log::info!("[XPath Fallback] /XPath — Strategy 2.5: FindAll(Descendants) raw tree search");
                    let desc_xpath = format!("//{}", xpath.trim_start_matches('/'));
                    if let Ok((r25, s25)) = find_by_xpath_raw_descendants(auto, window, &desc_xpath) {
                        if !r25.is_empty() {
                            log::info!("[XPath Fallback] ✓ Strategy 2.5: Found {} via raw descendant search ({}ms)", 
                                r25.len(), fallback_start.elapsed().as_millis());
                            return Ok((r25, s25));
                        }
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
            
            /// Find the byte offset where the first XPath step ends in the original string.
            /// E.g., for "/Pane[...]/Document[...]//Group[...]", returns the position right after "Pane[...]".
            /// This preserves `//` axis markers that would be lost by split/join.
            fn find_first_step_end(xpath: &str) -> usize {
                let bytes = xpath.as_bytes();
                let mut i = 0;
                // Skip leading slashes
                while i < bytes.len() && bytes[i] == b'/' {
                    i += 1;
                }
                // Now at start of first step content
                let mut bracket_depth: i32 = 0;
                while i < bytes.len() {
                    match bytes[i] {
                        b'[' => bracket_depth += 1,
                        b']' => {
                            if bracket_depth > 0 {
                                bracket_depth -= 1;
                            }
                        }
                        b'/' => {
                            if bracket_depth == 0 {
                                // End of first step (start of next / or //)
                                break;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
                i
            }
            
            /// Helper: given an element that matches the first XPath step, try to resolve
            /// the remaining steps via uiauto-xpath then raw tree walk.
            /// Returns per-segment validation results consistent with other strategies.
            ///
            /// `remaining_xpath` is the XPath after the first step, preserving `//` axis markers.
            /// `xpath_parts` is the split step list (for segment validation results only).
            fn try_remaining_from_match(
                auto: &IUIAutomation,
                match_elem: &IUIAutomationElement,
                remaining_xpath: &str,
                xpath_parts: &[&str],
                fallback_start: &std::time::Instant,
                strategy_label: &str,
            ) -> Option<(Vec<IUIAutomationElement>, Vec<SegmentValidationResult>)> {
                // If remaining path is empty, the matched element IS the result
                if remaining_xpath.is_empty() || remaining_xpath == "/" {
                    let duration_ms = fallback_start.elapsed().as_millis() as u64;
                    log::info!("[XPath Fallback] ✓ Strategy {}: Found element ({}ms)", strategy_label, duration_ms);
                    let segments: Vec<SegmentValidationResult> = xpath_parts.iter().enumerate().map(|(i, step)| {
                        SegmentValidationResult {
                            segment_index: i,
                            segment_text: step.to_string(),
                            matched: true,
                            match_count: if i == xpath_parts.len() - 1 { 1 } else { 0 },
                            duration_ms: if i == xpath_parts.len() - 1 { duration_ms } else { 0 },
                            predicate_failures: Vec::new(),
                        }
                    }).collect();
                    return Some((vec![match_elem.clone()], segments));
                }
                
                log::info!("[XPath Fallback] Strategy {}: trying remaining XPath from matched element: {}", strategy_label, remaining_xpath);
                
                // Try uiauto-xpath for the remaining path (preserves // descendant axes)
                if let Ok((matches, segments)) = find_by_xpath_detailed(auto, match_elem, remaining_xpath) {
                    if !matches.is_empty() {
                        log::info!("[XPath Fallback] ✓ Strategy {}: Found {} from subtree ({}ms)",
                            strategy_label, matches.len(), fallback_start.elapsed().as_millis());
                        // Prepend first-step segment and re-index
                        let mut all_segments = vec![SegmentValidationResult {
                            segment_index: 0,
                            segment_text: xpath_parts.first().unwrap_or(&"").to_string(),
                            matched: true,
                            match_count: 1,
                            duration_ms: 0,
                            predicate_failures: Vec::new(),
                        }];
                        for mut s in segments {
                            s.segment_index += 1;
                            all_segments.push(s);
                        }
                        return Some((matches, all_segments));
                    }
                }
                
                // Fallback: raw tree walk for remaining steps
                if let Ok(raw_walker) = unsafe { auto.RawViewWalker() } {
                    let remaining_parts: Vec<&str> = remaining_xpath.split('/').filter(|s| !s.is_empty()).collect();
                    if let Ok(matches) = walk_raw_tree_steps(auto, &raw_walker, match_elem, &remaining_parts) {
                        if !matches.is_empty() {
                            log::info!("[XPath Fallback] ✓ Strategy {}: Found {} via raw walk ({}ms)",
                                strategy_label, matches.len(), fallback_start.elapsed().as_millis());
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
                            return Some((matches, segments));
                        }
                    }
                }
                
                None
            }
            
            // Compute the remaining XPath after the first step, preserving // axis markers
            let first_step_end = find_first_step_end(xpath);
            let remaining_after_first = &xpath[first_step_end..];
            
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
                                if let Some(result) = try_remaining_from_match(auto, &child_elem, remaining_after_first, &xpath_parts, &fallback_start, "2.7a") {
                                    return Ok(result);
                                }
                            }
                            
                            // Also try: search inside this child HWND's subtree for the first step
                            if let Ok(raw_walker) = unsafe { auto.RawViewWalker() } {
                                let first_parsed = parse_xpath_step(xpath_parts[0]);
                                let mut sub_match = unsafe { raw_walker.GetFirstChildElement(&child_elem).ok() };
                                while let Some(sub) = sub_match {
                                    if element_matches_parsed_step(&sub, &first_parsed) {
                                        log::info!("[Strategy 2.7]   ✓ Found first-step match inside child HWND!");
                                        if let Some(result) = try_remaining_from_match(auto, &sub, remaining_after_first, &xpath_parts, &fallback_start, "2.7b") {
                                            return Ok(result);
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
                    if let Ok(walker) = unsafe { auto.RawViewWalker() }
                        .or_else(|_| unsafe { auto.ControlViewWalker() }) {
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
            
            // Strategy 4: Desktop root descendant search — DISABLED by default.
            //
            // Rationale: All searches in this system are window-scoped — we always locate the
            // target window first, then search within it. Desktop root search is fundamentally
            // incompatible with this design:
            //   1. It searches the entire OS UI tree, which can take minutes on large trees
            //      (e.g., Chrome/WebView with thousands of nodes).
            //   2. The "timeout" check is post-hoc — it only checks elapsed AFTER the query
            //      completes, so it doesn't actually prevent hangs.
            //   3. If Strategies 1-3b couldn't find the element within the correct window,
            //      a global search is extremely unlikely to help — the element probably
            //      doesn't exist anymore or the XPath is stale.
            //   4. Compass (relative) XPaths use /.., /preceding-sibling::, /following-sibling::
            //      which are meaningless when searched from Desktop root.
            //
            // To explicitly enable (for rare edge cases), set environment variable:
            //   ENABLE_DESKTOP_SEARCH=1
            let desktop_search_enabled = std::env::var("ENABLE_DESKTOP_SEARCH")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            
            if !desktop_search_enabled {
                log::info!("[XPath Fallback] /XPath — Strategy 4: SKIPPED (Desktop root search disabled by default, set ENABLE_DESKTOP_SEARCH=1 to enable)");
            } else {
                log::info!("[XPath Fallback] /XPath — Strategy 4: Desktop root descendant (explicitly enabled via ENABLE_DESKTOP_SEARCH=1)");
                let desktop_desc_xpath = format!("//{}", xpath.trim_start_matches('/'));
                if let Ok(desktop) = unsafe { auto.GetRootElement() } {
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

    /// Parse an XPath step string into its components.
    /// When `or` or `not()` predicates are detected, property lists are cleared
    /// so that callers fall back to uiauto-xpath for precise evaluation.
    /// Same-key attribute conflicts keep the last occurrence and emit a warning.
    fn parse_xpath_step(step: &str) -> ParsedXPathStep {
        let (type_name, predicates_str): (Option<String>, &str) = if step.starts_with('[') {
            (None, step)
        } else if let Some(bracket_pos) = step.find('[') {
            (Some(step[..bracket_pos].to_string()), &step[bracket_pos..])
        } else {
            (Some(step.to_string()), "")
        };

        // Detect `or` or `not()` — these cannot be reliably handled by simple
        // key=value matching; clear properties to force uiauto-xpath fallback.
        if predicates_str.contains(" or ") || predicates_str.contains("not(") {
            log::warn!("[parse_xpath_step] Detected 'or'/'not()' in predicates, skipping simple matching for: {}", step);
            return ParsedXPathStep {
                type_name,
                required_props: Vec::new(),
                require_starts_with: Vec::new(),
            };
        }

        let mut required_props: Vec<(String, String)> = Vec::new();
        let mut require_starts_with: Vec<(String, String)> = Vec::new();
        let mut seen_keys: std::collections::HashSet<String> = std::collections::HashSet::new();

        if let Ok(re) = regex::Regex::new(r#"@(\w+)='([^']*)'"#) {
            for cap in re.captures_iter(predicates_str) {
                if let (Some(key), Some(val)) = (cap.get(1), cap.get(2)) {
                    let k = key.as_str().to_string();
                    let v = val.as_str().to_string();
                    if seen_keys.contains(&k) {
                        log::warn!("[parse_xpath_step] Duplicate key '{}' in step '{}', keeping last value '{}'", k, step, v);
                        // Replace existing entry with the last value
                        if let Some(entry) = required_props.iter_mut().find(|(ek, _)| *ek == k) {
                            entry.1 = v.clone();
                        }
                    } else {
                        seen_keys.insert(k.clone());
                        required_props.push((k, v));
                    }
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

        // ── Diagnostic: print raw tree children at depth 1 and 2 (debug builds only) ──
        #[cfg(debug_assertions)]
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

        // BFS: search for first-step matches in the raw tree
        // Depth 8 covers most real-world UI hierarchies (e.g., WeChat's Qt tree can be 7+ levels deep).
        // Previously depth 3 was too shallow, causing elements captured by RawViewWalker to be
        // unreachable during validation/search.
        let mut queue: std::collections::VecDeque<(IUIAutomationElement, u32)> = std::collections::VecDeque::from(vec![(window.clone(), 0)]);
        let max_depth = 8u32;

        while let Some((elem, depth)) = queue.pop_front() {
            let mut child = unsafe { raw_walker.GetFirstChildElement(&elem).ok() };
            while let Some(c) = child {
                // Check if this child matches the first step
                if element_matches_parsed_step(&c, &first_step_parsed) {
                    first_step_matches.push(c.clone());
                }
                // Add to queue for deeper search if within depth limit
                if depth + 1 < max_depth {
                    queue.push_back((c.clone(), depth + 1));
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

        // Generate per-segment validation results for UI display.
        // Since uiauto-xpath executes the entire XPath at once, we split by `/`
        // to produce per-segment granularity consistent with Strategy 2.5/2.7 results.
        let parts: Vec<&str> = xpath.split('/').filter(|s| !s.is_empty()).collect();
        let segment_results: Vec<SegmentValidationResult> = if parts.is_empty() {
            vec![SegmentValidationResult {
                segment_index: 0,
                segment_text: xpath.to_string(),
                matched: !matches.is_empty(),
                match_count: matches.len(),
                duration_ms: total_duration_ms,
                predicate_failures: Vec::new(),
            }]
        } else {
            parts.iter().enumerate().map(|(i, step)| {
                SegmentValidationResult {
                    segment_index: i,
                    segment_text: step.to_string(),
                    matched: i < parts.len() - 1 || !matches.is_empty(),
                    match_count: if i == parts.len() - 1 { matches.len() } else { 0 },
                    duration_ms: if i == parts.len() - 1 { total_duration_ms } else { 0 },
                    predicate_failures: Vec::new(),
                }
            }).collect()
        };

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
                    element_info_from_uia(elem, window_rect.as_ref(), random_range, &mut rng)
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
            element_info_from_uia(elem, desktop_rect.as_ref(), random_range, &mut rng)
        }).collect()
    }

    /// Compass navigation: find base element by XPath, then walk the UIA tree step-by-step.
    ///
    /// This avoids the full XPath fallback pipeline for compass navigation,
    /// reducing latency from 4-6 seconds to <100ms.
    ///
    /// Returns the target element info, or an error string if navigation fails.
    pub fn navigate_from_element(
        window_selector: &str,
        base_xpath: &str,
        steps: &[crate::api::types::NavigateStep],
    ) -> Result<(Option<crate::api::types::ElementInfo>, String), String> {
        use crate::api::types::NavigateStep;

        let auto = get_automation().map_err(|e| format!("IUIAutomation init failed: {}", e))?;
        let windows = find_window_by_selector(&auto, window_selector);
        if windows.is_empty() {
            return Err(format!("Window not found: {}", window_selector));
        }

        // 窗口 XPath 前缀，用于构造 findSelector
        let _window_prefix = window_selector.to_string();

        // Phase 1: Find base element (one XPath search)
        let mut base_elem: Option<IUIAutomationElement> = None;
        let mut window_rect: Option<crate::api::types::Rect> = None;

        for window in &windows {
            let wr = unsafe {
                window.CurrentBoundingRectangle().ok().map(|r| {
                    crate::api::types::Rect { x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top }
                })
            };
            if let Ok((elements, _)) = find_by_xpath_with_fallback(&auto, window, base_xpath) {
                if let Some(elem) = elements.into_iter().next() {
                    base_elem = Some(elem);
                    window_rect = wr;
                    break;
                }
            }
        }

        let mut current = base_elem.ok_or_else(|| format!("Base element not found: {}", base_xpath))?;
        log::info!("[Compass] Base element found, applying {} steps", steps.len());

        // Phase 2: Walk tree step-by-step using TreeWalker
        // Prefer RawViewWalker for Chrome/WebView compatibility,
        // fall back to ControlViewWalker if RawViewWalker fails.
        let raw_walker = unsafe { auto.RawViewWalker().ok() };
        let ctrl_walker = unsafe { auto.ControlViewWalker().ok() };
        let walker = raw_walker.as_ref().or(ctrl_walker.as_ref())
            .ok_or_else(|| "Failed to create TreeWalker".to_string())?;

        for (i, step) in steps.iter().enumerate() {
            let before_label = format!("step[{}]={:?}", i, step);
            match step {
                NavigateStep::Parent { levels } => {
                    for lv in 0..*levels {
                        match unsafe { walker.GetParentElement(&current) } {
                            Ok(parent) => {
                                // Check if the parent is the desktop root (stop)
                                let desktop = unsafe { auto.GetRootElement().ok() };
                                let is_desktop = desktop.as_ref().map_or(false, |d| {
                                    unsafe { auto.CompareElements(&parent, d).unwrap_or(windows::core::BOOL(0)).as_bool() }
                                });
                                if is_desktop {
                                    return Err(format!("Step {} parent({}): reached Desktop root", i, lv + 1));
                                }
                                current = parent;
                            }
                            Err(e) => {
                                return Err(format!("Step {} parent({}): GetParent failed: {:?}", i, lv + 1, e));
                            }
                        }
                    }
                    log::info!("[Compass] {}: parent({}) OK", before_label, levels);
                }

                NavigateStep::Child { index } => {
                    let parent = current.clone();
                    // Enumerate all direct children
                    let mut children: Vec<IUIAutomationElement> = Vec::new();
                    let mut child = unsafe { walker.GetFirstChildElement(&parent).ok() };
                    while let Some(c) = child {
                        children.push(c.clone());
                        child = unsafe { walker.GetNextSiblingElement(&c).ok() };
                    }

                    let child_count = children.len() as i32;
                    let resolved = if *index >= 0 {
                        *index
                    } else {
                        // Negative index: -1 = last, -2 = second-to-last
                        child_count + *index
                    };

                    if resolved < 0 || resolved >= child_count {
                        return Err(format!(
                            "Step {} child({}): index out of range (resolved={}, count={})",
                            i, index, resolved, child_count
                        ));
                    }
                    current = children[resolved as usize].clone();
                    log::info!("[Compass] {}: child({}) OK (resolved={}, total={})", before_label, index, resolved, child_count);
                }

                NavigateStep::SiblingAbs { index } => {
                    // sibling_abs(N) = parent().child(N)
                    let parent = match unsafe { walker.GetParentElement(&current) } {
                        Ok(p) => p,
                        Err(e) => return Err(format!("Step {} sibling_abs({}): GetParent failed: {:?}", i, index, e)),
                    };
                    let mut siblings: Vec<IUIAutomationElement> = Vec::new();
                    let mut child = unsafe { walker.GetFirstChildElement(&parent).ok() };
                    while let Some(c) = child {
                        siblings.push(c.clone());
                        child = unsafe { walker.GetNextSiblingElement(&c).ok() };
                    }
                    let sib_count = siblings.len() as i32;
                    let resolved = if *index >= 0 { *index } else { sib_count + *index };
                    if resolved < 0 || resolved >= sib_count {
                        return Err(format!(
                            "Step {} sibling_abs({}): index out of range (resolved={}, count={})",
                            i, index, resolved, sib_count
                        ));
                    }
                    current = siblings[resolved as usize].clone();
                    log::info!("[Compass] {}: sibling_abs({}) OK", before_label, index);
                }

                NavigateStep::SiblingLeft { offset } => {
                    // preceding-sibling: go left by offset
                    for off in 1..=*offset {
                        match unsafe { walker.GetPreviousSiblingElement(&current) } {
                            Ok(prev) => current = prev,
                            Err(e) => return Err(format!(
                                "Step {} sibling_left({}): GetPreviousSibling at offset {} failed: {:?}",
                                i, offset, off, e
                            )),
                        }
                    }
                    log::info!("[Compass] {}: sibling_left({}) OK", before_label, offset);
                }

                NavigateStep::SiblingRight { offset } => {
                    // following-sibling: go right by offset
                    if *offset == 0 {
                        log::warn!("[Compass] Step {}: sibling_right(0) is a no-op", i);
                    }
                    for off in 1..=*offset {
                        match unsafe { walker.GetNextSiblingElement(&current) } {
                            Ok(next) => current = next,
                            Err(e) => return Err(format!(
                                "Step {} sibling_right({}): GetNextSibling at offset {} failed: {:?}",
                                i, offset, off, e
                            )),
                        }
                    }
                    log::info!("[Compass] {}: sibling_right({}) OK", before_label, offset);
                }
            }
        }

        // Phase 3: Convert result to ElementInfo
        let mut rng = rand::thread_rng();
        let info = element_info_from_uia(&current, window_rect.as_ref(), 5.0, &mut rng);

        // findSelector 由 SDK 本地构造（buildCompassXpath 等），后端不再从属性构建，
        // 因为属性构建的 XPath 无法定位 Chrome 内嵌元素等场景
        let find_selector = String::new();

        Ok((info, find_selector))
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

    // ═══════════════════════════════════════════════════════════════════════════
    // Inspect - 遍历元素子树，提取调试信息
    // ═══════════════════════════════════════════════════════════════════════════

    /// Inspect 返回的单个节点信息（核心模型）
    #[derive(Debug, Clone, serde::Serialize)]
    pub struct InspectNode {
        /// 元素层级深度（根元素为 0）
        pub depth: usize,
        /// 控件类型，如 "Button"、"Text"、"Edit" 等
        pub control_type: String,
        /// 控件的 Name 属性
        pub name: String,
        /// 控件的 ClassName 属性
        pub class_name: String,
        /// 控件的 AutomationId 属性
        pub automation_id: String,
        /// 控件的 FrameworkId 属性
        pub framework_id: String,
        /// 控件的文本内容（通过 ValuePattern 获取）
        pub text_value: Option<String>,
        /// 控件的 HelpText 属性（辅助说明文字）
        pub help_text: String,
        /// 控件的 ItemType 属性
        pub item_type: String,
        /// 控件的 ItemStatus 属性
        pub item_status: String,
        /// 控件的区域位置
        pub rect: Option<crate::api::types::Rect>,
        /// 是否在屏幕外
        pub is_offscreen: bool,
        /// 选中该控件相对于根元素的 XPath 表达式
        pub relative_xpath: String,
        /// 子节点列表
        pub children: Vec<InspectNode>,
    }

    /// Inspect 结果
    #[derive(Debug, Clone, serde::Serialize)]
    pub struct InspectResult {
        /// 是否成功
        pub success: bool,
        /// 根元素 XPath
        pub root_xpath: String,
        /// 结构化节点树
        pub nodes: Option<InspectNode>,
        /// 扁平化节点列表（DFS 顺序，无嵌套 children）
        pub flat_nodes: Vec<InspectNode>,
        /// 格式化文本（format='txt' 时有值）
        pub text_output: Option<String>,
        /// 子元素总数
        pub total_children: usize,
        /// 错误信息
        pub error: Option<String>,
    }

    /// 遍历指定元素下的所有子元素，提取层级/控件类型/name/Text/rect/相对xpath。
    ///
    /// 使用 RawViewWalker 递归遍历子树（支持 WebView/Chrome 元素），
    /// 对每个子元素提取关键属性并构建相对 XPath。
    pub fn inspect_subtree(
        window_selector: &str,
        element_xpath: &str,
        max_depth: usize,
        max_nodes: usize,
        format: &str,
    ) -> InspectResult {
        use std::time::Instant;
        let start = Instant::now();

        let auto = match get_automation() {
            Ok(a) => a,
            Err(e) => {
                return InspectResult {
                    success: false,
                    root_xpath: element_xpath.to_string(),
                    nodes: None,
                    flat_nodes: vec![],
                    text_output: None,
                    total_children: 0,
                    error: Some(format!("获取 IUIAutomation 实例失败: {}", e)),
                };
            }
        };

        // Step 1: 查找目标窗口
        let windows = find_window_by_selector(&auto, window_selector);
        if windows.is_empty() {
            return InspectResult {
                success: false,
                root_xpath: element_xpath.to_string(),
                nodes: None,
                flat_nodes: vec![],
                text_output: None,
                total_children: 0,
                error: Some(format!("窗口未找到: {}", window_selector)),
            };
        }

        // Step 2: 在窗口中查找目标元素
        let mut target_element: Option<IUIAutomationElement> = None;
        for window in &windows {
            if let Ok((elements, _)) = find_by_xpath_with_fallback(&auto, window, element_xpath) {
                if let Some(elem) = elements.into_iter().next() {
                    target_element = Some(elem);
                    break;
                }
            }
        }

        let root_element = match target_element {
            Some(e) => e,
            None => {
                return InspectResult {
                    success: false,
                    root_xpath: element_xpath.to_string(),
                    nodes: None,
                    flat_nodes: vec![],
                    text_output: None,
                    total_children: 0,
                    error: Some(format!("元素未找到: {}", element_xpath)),
                };
            }
        };

        // Step 3: 使用 RawViewWalker 递归遍历子树
        let raw_walker = match unsafe { auto.RawViewWalker() } {
            Ok(w) => w,
            Err(e) => {
                return InspectResult {
                    success: false,
                    root_xpath: element_xpath.to_string(),
                    nodes: None,
                    flat_nodes: vec![],
                    text_output: None,
                    total_children: 0,
                    error: Some(format!("获取 RawViewWalker 失败: {}", e)),
                };
            }
        };

        // 计数器，用于限制总节点数和跟踪同类型兄弟索引
        let mut total_count = 0usize;

        // DFS 遍历构建节点树
        let root_node = build_inspect_node(
            &root_element,
            &raw_walker,
            0,       // depth
            max_depth,
            max_nodes,
            &mut total_count,
            "",
        );

        log::info!(
            "[inspect_subtree] Completed in {}ms, total_nodes={}",
            start.elapsed().as_millis(),
            total_count,
        );

        // 生成扁平化节点列表
        let flat_nodes = flatten_inspect_tree(&root_node);

        // 根据格式生成输出
        let text_output = if format == "txt" || format == "text" {
            Some(format_inspect_tree(&root_node, 0))
        } else {
            None
        };

        InspectResult {
            success: true,
            root_xpath: element_xpath.to_string(),
            nodes: Some(root_node),
            flat_nodes,
            text_output,
            total_children: total_count.saturating_sub(1), // 减去根节点自身
            error: None,
        }
    }

    /// 递归构建 InspectNode 树
    ///
    /// 使用 DFS 遍历，对每个节点：
    /// - 提取属性（control_type, name, class_name, automation_id, framework_id, rect, is_offscreen）
    /// - 尝试获取 ValuePattern 的文本内容
    /// - 构建相对 XPath 路径
    /// - 递归处理子元素
    fn build_inspect_node(
        element: &IUIAutomationElement,
        walker: &IUIAutomationTreeWalker,
        depth: usize,
        max_depth: usize,
        max_nodes: usize,
        total_count: &mut usize,
        parent_xpath: &str,
    ) -> InspectNode {
        *total_count += 1;

        // 提取元素属性
        let control_type = unsafe { element.CurrentControlType() }
            .map(|id| control_type_name(id))
            .unwrap_or_default();
        let name = get_bstr(unsafe { element.CurrentName() });
        let class_name = get_bstr(unsafe { element.CurrentClassName() });
        let automation_id = get_bstr(unsafe { element.CurrentAutomationId() });
        let framework_id = get_bstr(unsafe { element.CurrentFrameworkId() });
        let help_text = get_bstr(unsafe { element.CurrentHelpText() });
        let item_type = get_bstr(unsafe { element.CurrentItemType() });
        let item_status = get_bstr(unsafe { element.CurrentItemStatus() });
        let is_offscreen = unsafe { element.CurrentIsOffscreen() }
            .map(|b| b.as_bool())
            .unwrap_or(false);

        // 获取边界矩形
        let rect = match unsafe { element.CurrentBoundingRectangle() } {
            Ok(r) => Some(crate::api::types::Rect {
                x: r.left,
                y: r.top,
                width: r.right - r.left,
                height: r.bottom - r.top,
            }),
            Err(_) => None,
        };

        // 尝试获取 ValuePattern 的文本内容
        let text_value = get_value_pattern_text(element);

        // 构建相对 XPath
        let relative_xpath = build_relative_xpath(
            &control_type,
            &name,
            &class_name,
            &automation_id,
            parent_xpath,
        );

        // 递归遍历子元素
        let mut children = Vec::new();
        if depth < max_depth && *total_count < max_nodes {
            // 首先收集所有直接子元素，以便计算同类型兄弟索引
            let child_elements: Vec<IUIAutomationElement> = {
                let mut kids = Vec::new();
                let mut child = unsafe { walker.GetFirstChildElement(element).ok() };
                while let Some(c) = child {
                    let last = c.clone();
                    kids.push(c);
                    child = unsafe { walker.GetNextSiblingElement(&last).ok() };
                }
                kids
            };

            // 按控件类型统计出现次数，用于构建带索引的 XPath
            let mut type_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            for child_elem in &child_elements {
                let ct = unsafe { child_elem.CurrentControlType() }
                    .map(|id| control_type_name(id))
                    .unwrap_or_default();
                let idx = type_counts.entry(ct.clone()).or_insert(0);
                *idx += 1;
            }

            // 跟踪当前同类型已出现的次数
            let mut type_seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

            for child_elem in &child_elements {
                if *total_count >= max_nodes {
                    break;
                }
                let ct = unsafe { child_elem.CurrentControlType() }
                    .map(|id| control_type_name(id))
                    .unwrap_or_default();

                let seen = type_seen.entry(ct.clone()).or_insert(0);
                *seen += 1;
                let same_type_total = type_counts.get(&ct).copied().unwrap_or(1);

                // 为子节点构建带索引的 XPath 前缀
                let child_xpath = if same_type_total > 1 {
                    format!("{}/{}[{}]", relative_xpath, ct, seen)
                } else {
                    format!("{}/{}", relative_xpath, ct)
                };

                let mut child_node = build_inspect_node_inner(
                    child_elem,
                    walker,
                    depth + 1,
                    max_depth,
                    max_nodes,
                    total_count,
                    &child_xpath,
                    &name,
                    &class_name,
                    &automation_id,
                );
                child_node.relative_xpath = child_xpath;
                children.push(child_node);
            }
        }

        InspectNode {
            depth,
            control_type,
            name,
            class_name,
            automation_id,
            framework_id,
            text_value,
            help_text,
            item_type,
            item_status,
            rect,
            is_offscreen,
            relative_xpath: relative_xpath.to_string(),
            children,
        }
    }

    /// 内部递归构建函数（由 build_inspect_node 调用用于子节点）
    ///
    /// 与 build_inspect_node 类似，但 XPath 已经由父节点构建好了
    fn build_inspect_node_inner(
        element: &IUIAutomationElement,
        walker: &IUIAutomationTreeWalker,
        depth: usize,
        max_depth: usize,
        max_nodes: usize,
        total_count: &mut usize,
        current_xpath: &str,
        _parent_name: &str,
        _parent_class: &str,
        _parent_aid: &str,
    ) -> InspectNode {
        *total_count += 1;

        let control_type = unsafe { element.CurrentControlType() }
            .map(|id| control_type_name(id))
            .unwrap_or_default();
        let name = get_bstr(unsafe { element.CurrentName() });
        let class_name = get_bstr(unsafe { element.CurrentClassName() });
        let automation_id = get_bstr(unsafe { element.CurrentAutomationId() });
        let framework_id = get_bstr(unsafe { element.CurrentFrameworkId() });
        let help_text = get_bstr(unsafe { element.CurrentHelpText() });
        let item_type = get_bstr(unsafe { element.CurrentItemType() });
        let item_status = get_bstr(unsafe { element.CurrentItemStatus() });
        let is_offscreen = unsafe { element.CurrentIsOffscreen() }
            .map(|b| b.as_bool())
            .unwrap_or(false);

        let rect = match unsafe { element.CurrentBoundingRectangle() } {
            Ok(r) => Some(crate::api::types::Rect {
                x: r.left,
                y: r.top,
                width: r.right - r.left,
                height: r.bottom - r.top,
            }),
            Err(_) => None,
        };

        let text_value = get_value_pattern_text(element);

        // 递归遍历子元素
        let mut children = Vec::new();
        if depth < max_depth && *total_count < max_nodes {
            let child_elements: Vec<IUIAutomationElement> = {
                let mut kids = Vec::new();
                let mut child = unsafe { walker.GetFirstChildElement(element).ok() };
                while let Some(c) = child {
                    let last = c.clone();
                    kids.push(c);
                    if kids.len() >= max_nodes {
                        break;
                    }
                    child = unsafe { walker.GetNextSiblingElement(&last).ok() };
                }
                kids
            };

            let mut type_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            for child_elem in &child_elements {
                let ct = unsafe { child_elem.CurrentControlType() }
                    .map(|id| control_type_name(id))
                    .unwrap_or_default();
                let idx = type_counts.entry(ct.clone()).or_insert(0);
                *idx += 1;
            }

            let mut type_seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

            for child_elem in &child_elements {
                if *total_count >= max_nodes {
                    break;
                }
                let ct = unsafe { child_elem.CurrentControlType() }
                    .map(|id| control_type_name(id))
                    .unwrap_or_default();

                let seen = type_seen.entry(ct.clone()).or_insert(0);
                *seen += 1;
                let same_type_total = type_counts.get(&ct).copied().unwrap_or(1);

                let child_xpath = if same_type_total > 1 {
                    format!("{}/{}[{}]", current_xpath, ct, seen)
                } else {
                    format!("{}/{}", current_xpath, ct)
                };

                let mut child_node = build_inspect_node_inner(
                    child_elem,
                    walker,
                    depth + 1,
                    max_depth,
                    max_nodes,
                    total_count,
                    &child_xpath,
                    &name,
                    &class_name,
                    &automation_id,
                );
                child_node.relative_xpath = child_xpath;
                children.push(child_node);
            }
        }

        InspectNode {
            depth,
            control_type,
            name,
            class_name,
            automation_id,
            framework_id,
            text_value,
            help_text,
            item_type,
            item_status,
            rect,
            is_offscreen,
            relative_xpath: String::new(), // 将由调用者设置
            children,
        }
    }

    /// 通过 ValuePattern 获取元素的文本内容
    fn get_value_pattern_text(element: &IUIAutomationElement) -> Option<String> {
        use windows::Win32::UI::Accessibility::{UIA_ValuePatternId, IValueProvider};

        let pattern = unsafe { element.GetCurrentPattern(UIA_ValuePatternId) }.ok()?;
        let value_provider: IValueProvider = pattern.cast().ok()?;
        let value = unsafe { value_provider.Value() }.ok()?;
        let bstr: BSTR = value.into();
        let s = bstr.to_string();
        if s.is_empty() { None } else { Some(s) }
    }

    /// 构建元素的相对 XPath 路径
    ///
    /// 策略：优先使用 AutomationId（最稳定），其次 ClassName，最后 Name
    fn build_relative_xpath(
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

    /// 将 InspectNode 树格式化为缩进文本
    fn format_inspect_tree(node: &InspectNode, indent: usize) -> String {
        let mut lines = Vec::new();
        format_inspect_node_recursive(node, indent, &mut lines);
        lines.join("\n")
    }

    fn format_inspect_node_recursive(node: &InspectNode, indent: usize, lines: &mut Vec<String>) {
        // 判断节点是否有可识别信息：仅 name、text_value、help_text，且必须是非空有效字符串
        let has_identifiable_info = !node.name.is_empty()
            || node.text_value.as_ref().map_or(false, |s| !s.is_empty())
            || !node.help_text.is_empty();

        // 仅当有可识别信息时才输出该节点（否则只递归处理子节点）
        if has_identifiable_info {
            let prefix = "  ".repeat(indent);

            let mut parts = vec![format!("{}{}", prefix, node.control_type)];
            if !node.name.is_empty() {
                parts.push(format!("name=\"{}\"", node.name));
            }
            if !node.class_name.is_empty() {
                parts.push(format!("class=\"{}\"", node.class_name));
            }
            if !node.automation_id.is_empty() {
                parts.push(format!("id=\"{}\"", node.automation_id));
            }
            if let Some(ref text) = node.text_value {
                parts.push(format!("text=\"{}\"", text));
            }
            if !node.help_text.is_empty() {
                parts.push(format!("help=\"{}\"", node.help_text));
            }
            if !node.item_type.is_empty() {
                parts.push(format!("itemType=\"{}\"", node.item_type));
            }
            if !node.item_status.is_empty() {
                parts.push(format!("itemStatus=\"{}\"", node.item_status));
            }
            if let Some(ref rect) = node.rect {
                parts.push(format!("rect=({},{},{},{})", rect.x, rect.y, rect.width, rect.height));
            }
            if node.is_offscreen {
                parts.push("[offscreen]".to_string());
            }

            lines.push(parts.join(" "));

            for child in &node.children {
                format_inspect_node_recursive(child, indent + 1, lines);
            }
        } else {
            // 无可识别信息的节点不显示，但其子节点继承当前缩进层级
            for child in &node.children {
                format_inspect_node_recursive(child, indent, lines);
            }
        }
    }

    /// 将嵌套的 InspectNode 树扁平化为 DFS 顺序的一维数组
    /// 每个节点的 children 字段置空（扁平列表不需要嵌套）
    fn flatten_inspect_tree(root: &InspectNode) -> Vec<InspectNode> {
        let mut result = Vec::new();
        flatten_inspect_node_recursive(root, &mut result);
        result
    }

    fn flatten_inspect_node_recursive(node: &InspectNode, result: &mut Vec<InspectNode>) {
        let has_identifiable_info = !node.name.is_empty()
            || node.text_value.as_ref().map_or(false, |s| !s.is_empty())
            || !node.help_text.is_empty();

        if has_identifiable_info {
            let mut flat = node.clone();
            flat.children = vec![];
            result.push(flat);
        }
        for child in &node.children {
            flatten_inspect_node_recursive(child, result);
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // Unit Tests — Enhanced Capture Element Selection Logic
    // ═════════════════════════════════════════════════════════════════════════
    #[cfg(test)]
    mod tests {
        use super::*;

        // ─── point_in_rect tests ─────────────────────────────────────────────

        #[test]
        fn test_point_in_rect_basic() {
            let rect = RECT { left: 100, top: 200, right: 300, bottom: 400 };
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
            let rect = RECT { left: 50, top: 50, right: 50, bottom: 50 };
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

        fn select_best_findall(candidates: &[CandidateElement]) -> Option<&CandidateElement> {
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
            let group_rect = RECT { left: 400, top: 250, right: 700, bottom: 290 };
            let text_rect = RECT { left: 400, top: 260, right: 700, bottom: 280 };
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
