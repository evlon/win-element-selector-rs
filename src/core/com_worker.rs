// src/core/com_worker.rs
//
// COM 专用工作线程 - 统一管理所有 UIA 操作
// 
// 架构设计：
// 1. 创建专用的 STA 线程持有 IUIAutomation 实例
// 2. 其他线程通过消息队列发送请求
// 3. 工作线程串行处理所有 UIA 操作
// 4. 统一的错误处理和状态恢复

use std::sync::mpsc::{self, Sender, Receiver};
use std::thread;
use std::time::Duration;

use crate::core::model::{CaptureResult, DetailedValidationResult};
use crate::api::types::ElementInfo;
use crate::core::uia::InspectResult;

/// 计算两个 Rect 的交集，无交集返回 None
fn intersect_rects(a: &crate::api::types::Rect, b: &crate::api::types::Rect) -> Option<crate::api::types::Rect> {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    if right > left && bottom > top {
        Some(crate::api::types::Rect {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        })
    } else {
        None
    }
}

/// UIA 操作请求类型
#[derive(Debug)]
pub enum UiaRequest {
    /// 捕获指定坐标的元素
    CaptureAt {
        x: i32,
        y: i32,
        response: Sender<anyhow::Result<CaptureResult>>,
    },

    /// 增强捕获：RawViewWalker + RECT 命中测试
    CaptureEnhancedAt {
        x: i32,
        y: i32,
        response: Sender<anyhow::Result<CaptureResult>>,
    },

    /// 查找元素
    FindElement {
        window_selector: String,
        xpath: String,
        random_range: Option<f32>,
        response: Sender<anyhow::Result<Vec<ElementInfo>>>,
    },
    
    /// 验证 XPath
    ValidateXPath {
        window_selector: String,
        element_xpath: String,
        hierarchy: Vec<crate::core::model::HierarchyNode>,
        response: Sender<anyhow::Result<DetailedValidationResult>>,
    },
    
    /// 查找相似元素（基于样本集）
    FindSimilarElements {
        samples: Vec<crate::core::model::SimilarElementSample>,
        threshold: f32,
        response: Sender<anyhow::Result<Vec<crate::core::model::CaptureResult>>>,
    },

    /// 查找共同元素（基于共同祖先链 XPath）
    FindCommonElements {
        window_selector: String,
        xpath: String,
        response: Sender<anyhow::Result<Vec<crate::api::types::ElementInfo>>>,
    },

    /// 检查窗口是否存在
    ExistsWindow {
        window_selector: String,
        response: Sender<anyhow::Result<bool>>,
    },

    /// 激活窗口
    ActivateWindow {
        window_selector: String,
        response: Sender<anyhow::Result<bool>>,
    },

    /// 激活窗口并聚焦元素
    ActivateAndFocusElement {
        window_selector: String,
        xpath: String,
        response: Sender<anyhow::Result<bool>>,
    },

    /// 列出窗口
    ListWindows {
        response: Sender<anyhow::Result<Vec<crate::core::model::WindowInfo>>>,
    },

    /// 获取元素可视区域位置信息
    GetElementVisibility {
        window_selector: String,
        element_xpath: String,
        container_xpath: Option<String>,
        response: Sender<anyhow::Result<crate::api::types::ElementVisibilityResponse>>,
    },

    /// 获取指定坐标处元素的边界矩形（轻量级，仅返回 rect）
    GetElementRectAtPoint {
        x: i32,
        y: i32,
        response: Sender<anyhow::Result<Option<crate::core::model::ElementRect>>>,
    },

    /// Inspect: 遍历元素子树提取调试信息
    Inspect {
        window_selector: String,
        element_xpath: String,
        max_depth: usize,
        max_nodes: usize,
        format: String,
        response: Sender<anyhow::Result<InspectResult>>,
    },

    /// 关闭工作线程
    Shutdown,
}

/// COM 工作线程管理器
pub struct ComWorker {
    sender: Option<Sender<UiaRequest>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl ComWorker {
    /// 创建并启动 COM 工作线程
    pub fn new() -> anyhow::Result<Self> {
        let (sender, receiver) = mpsc::channel::<UiaRequest>();
        
        let handle = thread::Builder::new()
            .name("com-worker".to_string())
            .spawn(move || {
                // 【关键修复】捕获 panic，防止后台线程崩溃导致整个程序退出
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    Self::worker_loop(receiver);
                }));
                
                if let Err(panic_info) = result {
                    log::error!("COM worker thread panicked: {:?}", panic_info);
                }
            })?;
        
        Ok(Self {
            sender: Some(sender),
            handle: Some(handle),
        })
    }
    
    /// 工作线程主循环
    fn worker_loop(receiver: Receiver<UiaRequest>) {
        use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
        
        // 初始化 COM STA
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if hr == windows::core::HRESULT(0) || hr == windows::core::HRESULT(1) {
            log::info!("COM worker thread initialized in STA mode");
        } else {
            log::error!("Failed to initialize COM in worker thread: HRESULT={:#010x}", hr.0 as u32);
            return;
        }
        
        // 创建 IUIAutomation 实例（单例，整个线程生命周期复用）
        let automation = match Self::create_automation() {
            Ok(auto) => {
                log::info!("IUIAutomation instance created in worker thread");
                auto
            }
            Err(e) => {
                log::error!("Failed to create IUIAutomation: {}", e);
                unsafe { CoUninitialize() };
                return;
            }
        };
        
        // 主循环：处理请求
        loop {
            match receiver.recv_timeout(Duration::from_secs(1)) {
                Ok(UiaRequest::Shutdown) => {
                    log::info!("COM worker thread shutting down");
                    break;
                }
                Ok(request) => {
                    Self::handle_request(&automation, request);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // 超时，继续等待
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    log::warn!("COM worker thread: all senders disconnected");
                    break;
                }
            }
        }
        
        // 清理
        drop(automation);
        unsafe { CoUninitialize() };
        log::info!("COM worker thread exited");
    }
    
    /// 创建 IUIAutomation 实例
    fn create_automation() -> anyhow::Result<windows::Win32::UI::Accessibility::IUIAutomation> {
        use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
        use windows::Win32::UI::Accessibility::CUIAutomation;
        
        let auto: windows::Win32::UI::Accessibility::IUIAutomation = unsafe {
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
        }?;
        
        Ok(auto)
    }
    
    /// 处理单个请求
    fn handle_request(
        automation: &windows::Win32::UI::Accessibility::IUIAutomation,
        request: UiaRequest,
    ) {
        match request {
            UiaRequest::CaptureAt { x, y, response } => {
                let start = std::time::Instant::now();
                log::debug!("[PERF] capture_at started for ({}, {})", x, y);
                let result = Self::do_capture(automation, x, y);
                log::debug!("[PERF] capture_at completed in {}ms", start.elapsed().as_millis());
                let _ = response.send(result);
            }
            UiaRequest::CaptureEnhancedAt { x, y, response } => {
                let start = std::time::Instant::now();
                log::info!("[PERF] capture_enhanced_at started for ({}, {})", x, y);
                let result = Self::do_capture_enhanced(automation, x, y);
                log::info!("[PERF] capture_enhanced_at completed in {}ms", start.elapsed().as_millis());
                let _ = response.send(result);
            }
            UiaRequest::FindElement { window_selector, xpath, random_range, response } => {
                let start = std::time::Instant::now();
                log::debug!("[PERF] find_element started");
                let result = Self::do_find_element(automation, &window_selector, &xpath, random_range);
                log::debug!("[PERF] find_element completed in {}ms", start.elapsed().as_millis());
                let _ = response.send(result);
            }
            UiaRequest::ValidateXPath { window_selector, element_xpath, hierarchy, response } => {
                let start = std::time::Instant::now();
                log::info!("[PERF] validate_xpath started");
                let result = Self::do_validate(automation, &window_selector, &element_xpath, &hierarchy);
                log::info!("[PERF] validate_xpath completed in {}ms", start.elapsed().as_millis());
                let _ = response.send(result);
            }
            UiaRequest::FindSimilarElements { samples, threshold, response } => {
                let start = std::time::Instant::now();
                log::info!("[PERF] find_similar_elements started ({} samples)", samples.len());
                let result = Self::do_find_similar_elements(automation, samples, threshold);
                log::info!("[PERF] find_similar_elements completed in {}ms, found {} elements",
                    start.elapsed().as_millis(), result.as_ref().map_or(0, |v| v.len()));
                let _ = response.send(result);
            }
            UiaRequest::FindCommonElements { window_selector, xpath, response } => {
                let start = std::time::Instant::now();
                log::info!("[PERF] find_common_elements started for {}", xpath);
                let result = Self::do_find_common_elements(automation, &window_selector, &xpath);
                log::info!("[PERF] find_common_elements completed in {}ms, found {} elements",
                    start.elapsed().as_millis(), result.as_ref().map_or(0, |v| v.len()));
                let _ = response.send(result);
            }
            UiaRequest::ExistsWindow { window_selector, response } => {
                let start = std::time::Instant::now();
                log::debug!("[PERF] exists_window started for {}", window_selector);
                let result = Self::do_exists_window(automation, &window_selector);
                log::debug!("[PERF] exists_window completed in {}ms", start.elapsed().as_millis());
                let _ = response.send(result);
            }
            UiaRequest::ActivateWindow { window_selector, response } => {
                let start = std::time::Instant::now();
                log::debug!("[PERF] activate_window started for {}", window_selector);
                let result = Self::do_activate_window(automation, &window_selector);
                log::debug!("[PERF] activate_window completed in {}ms", start.elapsed().as_millis());
                let _ = response.send(result);
            }
            UiaRequest::ActivateAndFocusElement { window_selector, xpath, response } => {
                let start = std::time::Instant::now();
                log::debug!("[PERF] activate_and_focus_element started for {} / {}", window_selector, xpath);
                let result = Self::do_activate_and_focus_element(automation, &window_selector, &xpath);
                log::debug!("[PERF] activate_and_focus_element completed in {}ms", start.elapsed().as_millis());
                let _ = response.send(result);
            }
            UiaRequest::ListWindows { response } => {
                let start = std::time::Instant::now();
                log::debug!("[PERF] list_windows started");
                let result = Self::do_list_windows();
                log::debug!("[PERF] list_windows completed in {}ms, found {} windows",
                    start.elapsed().as_millis(), result.as_ref().map_or(0, |v| v.len()));
                let _ = response.send(result);
            }
            UiaRequest::GetElementVisibility { window_selector, element_xpath, container_xpath, response } => {
                let start = std::time::Instant::now();
                log::info!("[PERF] get_element_visibility started for {}", element_xpath);
                let result = Self::do_get_element_visibility(automation, &window_selector, &element_xpath, container_xpath.as_deref());
                log::info!("[PERF] get_element_visibility completed in {}ms", start.elapsed().as_millis());
                let _ = response.send(result);
            }
            UiaRequest::GetElementRectAtPoint { x, y, response } => {
                let start = std::time::Instant::now();
                log::debug!("[PERF] get_element_rect_at_point started for ({}, {})", x, y);
                let result = Self::do_get_element_rect_at_point(automation, x, y);
                log::debug!("[PERF] get_element_rect_at_point completed in {}ms", start.elapsed().as_millis());
                let _ = response.send(result);
            }
            UiaRequest::Inspect { window_selector, element_xpath, max_depth, max_nodes, format, response } => {
                let start = std::time::Instant::now();
                log::info!("[PERF] inspect started for {} / {}", window_selector, element_xpath);
                let result = Self::do_inspect(automation, &window_selector, &element_xpath, max_depth, max_nodes, &format);
                log::info!("[PERF] inspect completed in {}ms", start.elapsed().as_millis());
                let _ = response.send(result);
            }
            UiaRequest::Shutdown => {
                // 已在主循环中处理
            }
        }
    }
    
    /// 执行捕获操作
    fn do_capture(
        _automation: &windows::Win32::UI::Accessibility::IUIAutomation,
        x: i32,
        y: i32,
    ) -> anyhow::Result<CaptureResult> {
        Ok(crate::core::uia::capture_at_point(x, y))
    }

    /// 执行增强捕获操作
    fn do_capture_enhanced(
        _automation: &windows::Win32::UI::Accessibility::IUIAutomation,
        x: i32,
        y: i32,
    ) -> anyhow::Result<CaptureResult> {
        Ok(crate::core::uia::capture_enhanced_at_point(x, y))
    }

    /// 执行查找操作
    fn do_find_element(
        _automation: &windows::Win32::UI::Accessibility::IUIAutomation,
        window_selector: &str,
        xpath: &str,
        random_range: Option<f32>,
    ) -> anyhow::Result<Vec<ElementInfo>> {
        // 【关键修复】直接调用 uia.rs 的底层函数，避免递归
        let results = crate::core::uia::find_all_elements_detailed(
            window_selector,
            xpath,
            random_range.unwrap_or(5.0),
        );
        Ok(results)
    }
    
    /// 执行验证操作
    fn do_validate(
        _automation: &windows::Win32::UI::Accessibility::IUIAutomation,
        window_selector: &str,
        element_xpath: &str,
        hierarchy: &[crate::core::model::HierarchyNode],
    ) -> anyhow::Result<DetailedValidationResult> {
        // 【关键修复】直接调用 uia.rs 的底层函数，避免递归
        Ok(crate::core::uia::validate_selector_and_xpath_detailed(
            window_selector,
            element_xpath,
            hierarchy,
        ))
    }
    
    /// 查找相似元素
    fn do_find_similar_elements(
        _automation: &windows::Win32::UI::Accessibility::IUIAutomation,
        samples: Vec<crate::core::model::SimilarElementSample>,
        threshold: f32,
    ) -> anyhow::Result<Vec<crate::core::model::CaptureResult>> {
        use crate::core::similarity;
        
        log::info!("[ComWorker] 开始查找相似元素，样本数: {}, 阈值: {}", samples.len(), threshold);
        
        if samples.is_empty() {
            return Ok(vec![]);
        }
        
        // 计算所有样本对的相似度
        let mut similar_pairs = vec![];
        for i in 0..samples.len() {
            for j in (i+1)..samples.len() {
                let sim = similarity::calculate_overall_similarity(&samples[i], &samples[j]);
                log::debug!("[ComWorker] 样本 {} vs {}: 相似度 = {:.3}", i+1, j+1, sim);
                
                if sim >= threshold {
                    similar_pairs.push((i, j, sim));
                    log::info!("[ComWorker] ✓ 发现相似对: 样本 {} 和 {} (相似度: {:.3})", i+1, j+1, sim);
                }
            }
        }
        
        log::info!("[ComWorker] 共找到 {} 个相似对", similar_pairs.len());
        
        // 将相似对转换为 CaptureResult
        // 策略：对于每个相似对，将两个样本都作为结果返回
        let mut results = vec![];
        let mut added_indices = std::collections::HashSet::new();
        
        for (i, j, _sim) in &similar_pairs {
            // 添加第一个样本（如果还没添加）
            if added_indices.insert(*i) {
                let sample = &samples[*i];
                let node = &sample.hierarchy_node;
                
                // 提取窗口信息（从祖先链的第一个节点）
                let window_info = sample.ancestor_chain.first().map(|ancestor| {
                    crate::core::model::WindowInfo {
                        title: ancestor.name.clone(),
                        class_name: ancestor.class_name.clone(),
                        process_id: ancestor.process_id,
                        process_name: String::new(), // TODO: 获取进程名
                    }
                });
                
                results.push(crate::core::model::CaptureResult {
                    hierarchy: sample.ancestor_chain.clone(),
                    cursor_x: node.rect.x,
                    cursor_y: node.rect.y,
                    error: None,
                    window_info,
                });
                
                log::debug!("[ComWorker] 添加样本 {} 到结果集", i + 1);
            }
            
            // 添加第二个样本（如果还没添加）
            if added_indices.insert(*j) {
                let sample = &samples[*j];
                let node = &sample.hierarchy_node;
                
                // 提取窗口信息（从祖先链的第一个节点）
                let window_info = sample.ancestor_chain.first().map(|ancestor| {
                    crate::core::model::WindowInfo {
                        title: ancestor.name.clone(),
                        class_name: ancestor.class_name.clone(),
                        process_id: ancestor.process_id,
                        process_name: String::new(), // TODO: 获取进程名
                    }
                });
                
                results.push(crate::core::model::CaptureResult {
                    hierarchy: sample.ancestor_chain.clone(),
                    cursor_x: node.rect.x,
                    cursor_y: node.rect.y,
                    error: None,
                    window_info,
                });
                
                log::debug!("[ComWorker] 添加样本 {} 到结果集", j + 1);
            }
        }
        
        log::info!("[ComWorker] 转换完成，返回 {} 个相似元素结果", results.len());
        Ok(results)
    }

    /// 查找共同元素（基于共同祖先链 XPath）
    fn do_find_common_elements(
        _automation: &windows::Win32::UI::Accessibility::IUIAutomation,
        _window_selector: &str,
        xpath: &str,
    ) -> anyhow::Result<Vec<crate::api::types::ElementInfo>> {
        log::info!("[ComWorker] 开始查找共同元素 (root 搜索): xpath={}", xpath);

        // 跳过 window selector，直接从 Desktop 根节点搜索。
        // 混合应用（如微信 Qt+WebView）的 UIA 树可能跨多个窗口，
        // 使用 window selector 会导致找不到目标元素。
        let results = crate::core::uia::find_all_elements_from_root(xpath, 5.0);
        Ok(results)
    }

    /// 激活窗口
    /// 检查窗口是否存在
    fn do_exists_window(
        _automation: &windows::Win32::UI::Accessibility::IUIAutomation,
        window_selector: &str,
    ) -> anyhow::Result<bool> {
        Ok(crate::core::uia::exists_window_by_selector(window_selector))
    }

    fn do_activate_window(
        _automation: &windows::Win32::UI::Accessibility::IUIAutomation,
        window_selector: &str,
    ) -> anyhow::Result<bool> {
        Ok(crate::core::uia::activate_window_by_selector(window_selector))
    }

    /// 激活窗口并聚焦元素
    fn do_activate_and_focus_element(
        _automation: &windows::Win32::UI::Accessibility::IUIAutomation,
        window_selector: &str,
        xpath: &str,
    ) -> anyhow::Result<bool> {
        Ok(crate::core::uia::activate_and_focus_element(window_selector, xpath))
    }

    /// 列出所有窗口
    fn do_list_windows() -> anyhow::Result<Vec<crate::core::model::WindowInfo>> {
        Ok(crate::capture::list_windows())
    }

    /// 获取元素可视区域位置
    fn do_get_element_visibility(
        _automation: &windows::Win32::UI::Accessibility::IUIAutomation,
        window_selector: &str,
        element_xpath: &str,
        container_xpath: Option<&str>,
    ) -> anyhow::Result<crate::api::types::ElementVisibilityResponse> {
        use crate::api::types::{ElementVisibilityResponse, OverflowInfo, Rect};
        use crate::core::model::ValidationResult;

        // 1. 获取元素信息（rect + is_offscreen）
        let detailed = crate::core::uia::validate_selector_and_xpath_detailed(
            window_selector,
            element_xpath,
            &[],
        );

        let (element_rect, is_offscreen) = match &detailed.overall {
            ValidationResult::Found { first_rect, .. } => {
                let rect = first_rect.clone();
                let offscreen = detailed.is_offscreen;
                (rect, offscreen)
            }
            ValidationResult::NotFound => {
                return Ok(ElementVisibilityResponse {
                    found: false,
                    is_offscreen: None,
                    visibility: "not_found".to_string(),
                    position: "unknown".to_string(),
                    element_rect: None,
                    visible_rect: None,
                    viewport_rect: None,
                    overflow: None,
                    scroll_direction: None,
                    error: Some("元素未找到".to_string()),
                });
            }
            ValidationResult::Error(e) => {
                return Ok(ElementVisibilityResponse {
                    found: false,
                    is_offscreen: None,
                    visibility: "error".to_string(),
                    position: "unknown".to_string(),
                    element_rect: None,
                    visible_rect: None,
                    viewport_rect: None,
                    overflow: None,
                    scroll_direction: None,
                    error: Some(e.clone()),
                });
            }
            _ => {
                return Ok(ElementVisibilityResponse {
                    found: false,
                    is_offscreen: None,
                    visibility: "unknown".to_string(),
                    position: "unknown".to_string(),
                    element_rect: None,
                    visible_rect: None,
                    viewport_rect: None,
                    overflow: None,
                    scroll_direction: None,
                    error: Some("校验状态未知".to_string()),
                });
            }
        };

        let elem_rect = match &element_rect {
            Some(r) => r,
            None => {
                return Ok(ElementVisibilityResponse {
                    found: true,
                    is_offscreen,
                    visibility: "unknown".to_string(),
                    position: "unknown".to_string(),
                    element_rect: None,
                    visible_rect: None,
                    viewport_rect: None,
                    overflow: None,
                    scroll_direction: None,
                    error: Some("元素坐标获取失败".to_string()),
                });
            }
        };

        // 2. 获取窗口矩形作为视口
        let window_rect = crate::core::uia::get_window_rect_by_selector(window_selector);
        let viewport_rect = match &window_rect {
            Some(r) => r,
            None => {
                // 无法获取窗口矩形，仍然返回元素信息
                return Ok(ElementVisibilityResponse {
                    found: true,
                    is_offscreen,
                    visibility: if is_offscreen.unwrap_or(false) { "offscreen".to_string() } else { "visible".to_string() },
                    position: "unknown".to_string(),
                    element_rect: Some(Rect {
                        x: elem_rect.x,
                        y: elem_rect.y,
                        width: elem_rect.width,
                        height: elem_rect.height,
                    }),
                    visible_rect: None,
                    viewport_rect: None,
                    overflow: None,
                    scroll_direction: None,
                    error: Some("窗口矩形获取失败".to_string()),
                });
            }
        };

        // 3. 计算元素与视口的位置关系
        let elem_api_rect = Rect {
            x: elem_rect.x,
            y: elem_rect.y,
            width: elem_rect.width,
            height: elem_rect.height,
        };
        let vp_api_rect = Rect {
            x: viewport_rect.x,
            y: viewport_rect.y,
            width: viewport_rect.width,
            height: viewport_rect.height,
        };

        // 3.5 计算可见矩形 = 元素矩形 ∩ 容器矩形(可选) ∩ 窗口视口
        let clip_rect = if let Some(cxpath) = container_xpath {
            // 获取容器元素矩形
            let container_detailed = crate::core::uia::validate_selector_and_xpath_detailed(
                window_selector, cxpath, &[],
            );
            match &container_detailed.overall {
                ValidationResult::Found { first_rect: Some(cr), .. } => {
                    // 容器可见矩形 = 容器矩形 ∩ 视口矩形
                    let container_api_rect = Rect {
                        x: cr.x, y: cr.y, width: cr.width, height: cr.height,
                    };
                    intersect_rects(&container_api_rect, &vp_api_rect)
                }
                _ => {
                    // 容器查找失败，仅用视口
                    Some(vp_api_rect.clone())
                }
            }
        } else {
            Some(vp_api_rect.clone())
        };

        let visible_rect = match &clip_rect {
            Some(clip) => intersect_rects(&elem_api_rect, clip),
            None => None,
        };

        // 计算各方向溢出像素
        let overflow_top = (vp_api_rect.y - elem_api_rect.y).max(0);
        let overflow_bottom = ((elem_api_rect.y + elem_api_rect.height) - (vp_api_rect.y + vp_api_rect.height)).max(0);
        let overflow_left = (vp_api_rect.x - elem_api_rect.x).max(0);
        let overflow_right = ((elem_api_rect.x + elem_api_rect.width) - (vp_api_rect.x + vp_api_rect.width)).max(0);

        let has_overflow = overflow_top > 0 || overflow_bottom > 0 || overflow_left > 0 || overflow_right > 0;

        // 判断可视性
        let visibility = if !has_overflow {
            "fully_visible".to_string()
        } else if overflow_top > 0 && overflow_bottom > 0
            || overflow_left > 0 && overflow_right > 0
        {
            // 元素同时跨越视口上下（或左右），说明元素比视口大或完全在视口外
            "offscreen".to_string()
        } else {
            "partially_visible".to_string()
        };

        // 判断主位置方向
        let position = if !has_overflow {
            "inside".to_string()
        } else if overflow_top >= overflow_bottom && overflow_top >= overflow_left && overflow_top >= overflow_right {
            "above".to_string()
        } else if overflow_bottom >= overflow_top && overflow_bottom >= overflow_left && overflow_bottom >= overflow_right {
            "below".to_string()
        } else if overflow_left >= overflow_right {
            "left".to_string()
        } else {
            "right".to_string()
        };

        // 建议滚动方向
        let scroll_direction = if !has_overflow {
            None
        } else if overflow_top > overflow_bottom {
            Some("down".to_string()) // 元素在上方，需要向下滚
        } else if overflow_bottom > overflow_top {
            Some("up".to_string())   // 元素在下方，需要向上滚
        } else if overflow_left > overflow_right {
            Some("right".to_string())
        } else {
            Some("left".to_string())
        };

        Ok(ElementVisibilityResponse {
            found: true,
            is_offscreen,
            visibility,
            position,
            element_rect: Some(elem_api_rect),
            visible_rect,
            viewport_rect: Some(vp_api_rect),
            overflow: Some(OverflowInfo {
                top: overflow_top,
                bottom: overflow_bottom,
                left: overflow_left,
                right: overflow_right,
            }),
            scroll_direction,
            error: None,
        })
    }

    /// 获取指定坐标处元素的边界矩形（轻量级）
    fn do_get_element_rect_at_point(
        automation: &windows::Win32::UI::Accessibility::IUIAutomation,
        x: i32,
        y: i32,
    ) -> anyhow::Result<Option<crate::core::model::ElementRect>> {
        let pt = windows::Win32::Foundation::POINT { x, y };

        let element: windows::Win32::UI::Accessibility::IUIAutomationElement = unsafe {
            match automation.ElementFromPoint(pt) {
                Ok(e) => e,
                Err(e) => {
                    log::debug!("ElementFromPoint({}, {}) failed: {:?}", x, y, e);
                    return Ok(None);
                }
            }
        };

        match unsafe { element.CurrentBoundingRectangle() } {
            Ok(r) => Ok(Some(crate::core::model::ElementRect {
                x: r.left,
                y: r.top,
                width: r.right - r.left,
                height: r.bottom - r.top,
            })),
            Err(_) => Ok(None),
        }
    }

    /// 执行 Inspect 操作
    fn do_inspect(
        _automation: &windows::Win32::UI::Accessibility::IUIAutomation,
        window_selector: &str,
        element_xpath: &str,
        max_depth: usize,
        max_nodes: usize,
        format: &str,
    ) -> anyhow::Result<InspectResult> {
        Ok(crate::core::uia::inspect_subtree(
            window_selector,
            element_xpath,
            max_depth,
            max_nodes,
            format,
        ))
    }

    /// 发送捕获请求
    pub fn capture_at(&self, x: i32, y: i32) -> anyhow::Result<CaptureResult> {
        let (response_sender, response_receiver) = mpsc::channel();

        if let Some(ref sender) = self.sender {
            sender.send(UiaRequest::CaptureAt {
                x, y, response: response_sender,
            })?;
            response_receiver
                .recv_timeout(std::time::Duration::from_secs(10))
                .map_err(|e| anyhow::anyhow!("COM worker timeout after 10s: {:?}", e))?
        } else {
            Err(anyhow::anyhow!("COM worker not initialized"))
        }
    }

    /// 发送增强捕获请求
    pub fn capture_enhanced_at(&self, x: i32, y: i32) -> anyhow::Result<CaptureResult> {
        let (response_sender, response_receiver) = mpsc::channel();

        if let Some(ref sender) = self.sender {
            sender.send(UiaRequest::CaptureEnhancedAt {
                x, y, response: response_sender,
            })?;
            // 增强捕获可能更耗时（全树枚举），给 30 秒超时
            response_receiver
                .recv_timeout(std::time::Duration::from_secs(30))
                .map_err(|e| anyhow::anyhow!("COM worker enhanced capture timeout: {:?}", e))?
        } else {
            Err(anyhow::anyhow!("COM worker not initialized"))
        }
    }

    /// 发送查找请求
    pub fn find_element(
        &self,
        window_selector: String,
        xpath: String,
        random_range: Option<f32>,
    ) -> anyhow::Result<Vec<ElementInfo>> {
        let (response_sender, response_receiver) = mpsc::channel();
        
        if let Some(ref sender) = self.sender {
            sender.send(UiaRequest::FindElement {
                window_selector,
                xpath,
                random_range,
                response: response_sender,
            })?;
            
            // 【关键修复】添加超时机制
            response_receiver
                .recv_timeout(std::time::Duration::from_secs(10))
                .map_err(|e| anyhow::anyhow!("COM worker timeout after 10s: {:?}", e))?
        } else {
            Err(anyhow::anyhow!("COM worker not initialized"))
        }
    }
    
    /// 发送验证请求
    pub fn validate_xpath(
        &self,
        window_selector: String,
        element_xpath: String,
        hierarchy: Vec<crate::core::model::HierarchyNode>,
    ) -> anyhow::Result<DetailedValidationResult> {
        let (response_sender, response_receiver) = mpsc::channel();
        
        if let Some(ref sender) = self.sender {
            sender.send(UiaRequest::ValidateXPath {
                window_selector,
                element_xpath,
                hierarchy,
                response: response_sender,
            })?;
            
            // 【关键修复】添加超时机制（校验可能更耗时，给 15 秒）
            response_receiver
                .recv_timeout(std::time::Duration::from_secs(15))
                .map_err(|e| anyhow::anyhow!("COM worker validation timeout after 15s: {:?}", e))?
        } else {
            Err(anyhow::anyhow!("COM worker not initialized"))
        }
    }
    
    /// 发送相似元素查找请求
    pub fn find_similar_elements(
        &self,
        samples: Vec<crate::core::model::SimilarElementSample>,
        threshold: f32,
    ) -> anyhow::Result<Vec<crate::core::model::CaptureResult>> {
        let (response_sender, response_receiver) = mpsc::channel();
        
        if let Some(ref sender) = self.sender {
            sender.send(UiaRequest::FindSimilarElements {
                samples,
                threshold,
                response: response_sender,
            })?;
            
            // 【关键修复】添加超时机制（批量查找可能很耗时，给 30 秒）
            response_receiver
                .recv_timeout(std::time::Duration::from_secs(30))
                .map_err(|e| anyhow::anyhow!("COM worker similar elements search timeout after 30s: {:?}", e))?
        } else {
            Err(anyhow::anyhow!("COM worker not initialized"))
        }
    }

    /// 发送共同元素查找请求
    pub fn find_common_elements(
        &self,
        window_selector: String,
        xpath: String,
    ) -> anyhow::Result<Vec<crate::api::types::ElementInfo>> {
        let (response_sender, response_receiver) = mpsc::channel();

        if let Some(ref sender) = self.sender {
            sender.send(UiaRequest::FindCommonElements {
                window_selector,
                xpath,
                response: response_sender,
            })?;

            response_receiver
                .recv_timeout(std::time::Duration::from_secs(15))
                .map_err(|e| anyhow::anyhow!("COM worker common elements search timeout after 15s: {:?}", e))?
        } else {
            Err(anyhow::anyhow!("COM worker not initialized"))
        }
    }

    /// 检查窗口是否存在
    pub fn exists_window(&self, window_selector: String) -> anyhow::Result<bool> {
        let (response_sender, response_receiver) = mpsc::channel();

        if let Some(ref sender) = self.sender {
            sender.send(UiaRequest::ExistsWindow {
                window_selector,
                response: response_sender,
            })?;

            response_receiver
                .recv_timeout(std::time::Duration::from_secs(10))
                .map_err(|e| anyhow::anyhow!("COM worker exists_window timeout after 10s: {:?}", e))?
        } else {
            Err(anyhow::anyhow!("COM worker not initialized"))
        }
    }

    /// 激活窗口
    pub fn activate_window(&self, window_selector: String) -> anyhow::Result<bool> {
        let (response_sender, response_receiver) = mpsc::channel();

        if let Some(ref sender) = self.sender {
            sender.send(UiaRequest::ActivateWindow {
                window_selector,
                response: response_sender,
            })?;

            response_receiver
                .recv_timeout(std::time::Duration::from_secs(10))
                .map_err(|e| anyhow::anyhow!("COM worker activate_window timeout after 10s: {:?}", e))?
        } else {
            Err(anyhow::anyhow!("COM worker not initialized"))
        }
    }

    /// 激活窗口并聚焦元素
    pub fn activate_and_focus_element(&self, window_selector: String, xpath: String) -> anyhow::Result<bool> {
        let (response_sender, response_receiver) = mpsc::channel();

        if let Some(ref sender) = self.sender {
            sender.send(UiaRequest::ActivateAndFocusElement {
                window_selector,
                xpath,
                response: response_sender,
            })?;

            response_receiver
                .recv_timeout(std::time::Duration::from_secs(15))
                .map_err(|e| anyhow::anyhow!("COM worker activate_and_focus_element timeout after 15s: {:?}", e))?
        } else {
            Err(anyhow::anyhow!("COM worker not initialized"))
        }
    }

    /// 列出所有窗口
    pub fn list_windows(&self) -> anyhow::Result<Vec<crate::core::model::WindowInfo>> {
        let (response_sender, response_receiver) = mpsc::channel();

        if let Some(ref sender) = self.sender {
            sender.send(UiaRequest::ListWindows {
                response: response_sender,
            })?;

            response_receiver
                .recv_timeout(std::time::Duration::from_secs(10))
                .map_err(|e| anyhow::anyhow!("COM worker list_windows timeout after 10s: {:?}", e))?
        } else {
            Err(anyhow::anyhow!("COM worker not initialized"))
        }
    }

    /// 获取元素可视区域位置
    pub fn get_element_visibility(
        &self,
        window_selector: String,
        element_xpath: String,
        container_xpath: Option<String>,
    ) -> anyhow::Result<crate::api::types::ElementVisibilityResponse> {
        let (response_sender, response_receiver) = mpsc::channel();

        if let Some(ref sender) = self.sender {
            sender.send(UiaRequest::GetElementVisibility {
                window_selector,
                element_xpath,
                container_xpath,
                response: response_sender,
            })?;

            response_receiver
                .recv_timeout(std::time::Duration::from_secs(10))
                .map_err(|e| anyhow::anyhow!("COM worker get_element_visibility timeout after 10s: {:?}", e))?
        } else {
            Err(anyhow::anyhow!("COM worker not initialized"))
        }
    }

    /// 获取指定坐标处元素的边界矩形
    pub fn get_element_rect_at_point(
        &self,
        x: i32,
        y: i32,
    ) -> anyhow::Result<Option<crate::core::model::ElementRect>> {
        let (response_sender, response_receiver) = mpsc::channel();

        if let Some(ref sender) = self.sender {
            sender.send(UiaRequest::GetElementRectAtPoint {
                x, y, response: response_sender,
            })?;

            response_receiver
                .recv_timeout(std::time::Duration::from_secs(5))
                .map_err(|e| anyhow::anyhow!("COM worker get_element_rect_at_point timeout after 5s: {:?}", e))?
        } else {
            Err(anyhow::anyhow!("COM worker not initialized"))
        }
    }

    /// Inspect: 遍历元素子树提取调试信息
    pub fn inspect(
        &self,
        window_selector: String,
        element_xpath: String,
        max_depth: usize,
        max_nodes: usize,
        format: String,
    ) -> anyhow::Result<InspectResult> {
        let (response_sender, response_receiver) = mpsc::channel();

        if let Some(ref sender) = self.sender {
            sender.send(UiaRequest::Inspect {
                window_selector,
                element_xpath,
                max_depth,
                max_nodes,
                format,
                response: response_sender,
            })?;

            // Inspect 大子树可能很耗时，给 60 秒超时
            response_receiver
                .recv_timeout(std::time::Duration::from_secs(60))
                .map_err(|e| anyhow::anyhow!("COM worker inspect timeout after 60s: {:?}", e))?
        } else {
            Err(anyhow::anyhow!("COM worker not initialized"))
        }
    }

    /// 优雅关闭工作线程
    pub fn shutdown(&mut self) {
        if let Some(ref sender) = self.sender {
            let _ = sender.send(UiaRequest::Shutdown);
        }
        
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        
        self.sender = None;
    }
}

impl Drop for ComWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// 全局单例
use std::sync::OnceLock;

static COM_WORKER: OnceLock<std::sync::Mutex<Option<ComWorker>>> = OnceLock::new();

/// 获取全局 COM 工作线程实例
pub fn get_com_worker() -> &'static std::sync::Mutex<Option<ComWorker>> {
    COM_WORKER.get_or_init(|| std::sync::Mutex::new(None))
}

/// 初始化全局 COM 工作线程
pub fn init_global_com_worker() -> anyhow::Result<()> {
    let mut worker_opt = get_com_worker().lock().unwrap();
    if worker_opt.is_none() {
        *worker_opt = Some(ComWorker::new()?);
        log::info!("Global COM worker initialized");
    }
    Ok(())
}

/// 使用全局 COM 工作线程执行捕获
pub fn global_capture_at(x: i32, y: i32) -> anyhow::Result<CaptureResult> {
    let worker_opt = get_com_worker().lock().unwrap();
    if let Some(ref worker) = *worker_opt {
        worker.capture_at(x, y)
    } else {
        Err(anyhow::anyhow!("Global COM worker not initialized. Call init_global_com_worker() first."))
    }
}

/// 使用全局 COM 工作线程执行增强捕获（RawViewWalker + RECT 命中测试）
pub fn global_capture_enhanced_at(x: i32, y: i32) -> anyhow::Result<CaptureResult> {
    let worker_opt = get_com_worker().lock().unwrap();
    if let Some(ref worker) = *worker_opt {
        worker.capture_enhanced_at(x, y)
    } else {
        Err(anyhow::anyhow!("Global COM worker not initialized"))
    }
}

/// 使用全局 COM 工作线程查找元素
pub fn global_find_element(
    window_selector: String,
    xpath: String,
    random_range: Option<f32>,
) -> anyhow::Result<Vec<ElementInfo>> {
    let worker_opt = get_com_worker().lock().unwrap();
    if let Some(ref worker) = *worker_opt {
        worker.find_element(window_selector, xpath, random_range)
    } else {
        Err(anyhow::anyhow!("Global COM worker not initialized"))
    }
}

/// 使用全局 COM 工作线程验证 XPath
pub fn global_validate_xpath(
    window_selector: String,
    element_xpath: String,
    hierarchy: Vec<crate::core::model::HierarchyNode>,
) -> anyhow::Result<DetailedValidationResult> {
    let worker_opt = get_com_worker().lock().unwrap();
    if let Some(ref worker) = *worker_opt {
        worker.validate_xpath(window_selector, element_xpath, hierarchy)
    } else {
        Err(anyhow::anyhow!("Global COM worker not initialized"))
    }
}

/// 使用全局 COM 工作线程查找相似元素
pub fn global_find_similar_elements(
    samples: Vec<crate::core::model::SimilarElementSample>,
    threshold: f32,
) -> anyhow::Result<Vec<crate::core::model::CaptureResult>> {
    let worker_opt = get_com_worker().lock().unwrap();
    if let Some(ref worker) = *worker_opt {
        worker.find_similar_elements(samples, threshold)
    } else {
        Err(anyhow::anyhow!("Global COM worker not initialized"))
    }
}

/// 使用全局 COM 工作线程查找共同元素
pub fn global_find_common_elements(
    window_selector: String,
    xpath: String,
) -> anyhow::Result<Vec<crate::api::types::ElementInfo>> {
    let worker_opt = get_com_worker().lock().unwrap();
    if let Some(ref worker) = *worker_opt {
        worker.find_common_elements(window_selector, xpath)
    } else {
        Err(anyhow::anyhow!("Global COM worker not initialized"))
    }
}

/// 使用全局 COM 工作线程检查窗口是否存在
pub fn global_exists_window(window_selector: String) -> anyhow::Result<bool> {
    let worker_opt = get_com_worker().lock().unwrap();
    if let Some(ref worker) = *worker_opt {
        worker.exists_window(window_selector)
    } else {
        Err(anyhow::anyhow!("Global COM worker not initialized"))
    }
}

/// 使用全局 COM 工作线程激活窗口
pub fn global_activate_window(window_selector: String) -> anyhow::Result<bool> {
    let worker_opt = get_com_worker().lock().unwrap();
    if let Some(ref worker) = *worker_opt {
        worker.activate_window(window_selector)
    } else {
        Err(anyhow::anyhow!("Global COM worker not initialized"))
    }
}

/// 使用全局 COM 工作线程激活窗口并聚焦元素
pub fn global_activate_and_focus_element(window_selector: String, xpath: String) -> anyhow::Result<bool> {
    let worker_opt = get_com_worker().lock().unwrap();
    if let Some(ref worker) = *worker_opt {
        worker.activate_and_focus_element(window_selector, xpath)
    } else {
        Err(anyhow::anyhow!("Global COM worker not initialized"))
    }
}

/// 使用全局 COM 工作线程列出所有窗口
pub fn global_list_windows() -> anyhow::Result<Vec<crate::core::model::WindowInfo>> {
    let worker_opt = get_com_worker().lock().unwrap();
    if let Some(ref worker) = *worker_opt {
        worker.list_windows()
    } else {
        Err(anyhow::anyhow!("Global COM worker not initialized"))
    }
}

/// 使用全局 COM 工作线程获取元素可视区域位置
pub fn global_get_element_visibility(
    window_selector: String,
    element_xpath: String,
    container_xpath: Option<String>,
) -> anyhow::Result<crate::api::types::ElementVisibilityResponse> {
    let worker_opt = get_com_worker().lock().unwrap();
    if let Some(ref worker) = *worker_opt {
        worker.get_element_visibility(window_selector, element_xpath, container_xpath)
    } else {
        Err(anyhow::anyhow!("Global COM worker not initialized"))
    }
}

/// 使用全局 COM 工作线程获取指定坐标处元素的边界矩形
pub fn global_get_element_rect_at_point(x: i32, y: i32) -> anyhow::Result<Option<crate::core::model::ElementRect>> {
    let worker_opt = get_com_worker().lock().unwrap();
    if let Some(ref worker) = *worker_opt {
        worker.get_element_rect_at_point(x, y)
    } else {
        Err(anyhow::anyhow!("Global COM worker not initialized"))
    }
}

/// 使用全局 COM 工作线程执行 Inspect 操作
pub fn global_inspect(
    window_selector: String,
    element_xpath: String,
    max_depth: usize,
    max_nodes: usize,
    format: String,
) -> anyhow::Result<InspectResult> {
    let worker_opt = get_com_worker().lock().unwrap();
    if let Some(ref worker) = *worker_opt {
        worker.inspect(window_selector, element_xpath, max_depth, max_nodes, format)
    } else {
        Err(anyhow::anyhow!("Global COM worker not initialized"))
    }
}
