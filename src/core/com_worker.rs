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

use crate::core::model::{CaptureResult, DetailedValidationResult, WindowInfo};
use crate::api::types::ElementInfo;

/// UIA 操作请求类型
#[derive(Debug)]
pub enum UiaRequest {
    /// 捕获指定坐标的元素
    CaptureAt {
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
    
    /// 获取窗口列表
    EnumerateWindows {
        response: Sender<anyhow::Result<Vec<WindowInfo>>>,
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
                Self::worker_loop(receiver);
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
                let result = Self::do_capture(automation, x, y);
                let _ = response.send(result);
            }
            UiaRequest::FindElement { window_selector, xpath, random_range, response } => {
                let result = Self::do_find_element(automation, &window_selector, &xpath, random_range);
                let _ = response.send(result);
            }
            UiaRequest::ValidateXPath { window_selector, element_xpath, hierarchy, response } => {
                let result = Self::do_validate(automation, &window_selector, &element_xpath, &hierarchy);
                let _ = response.send(result);
            }
            UiaRequest::EnumerateWindows { response } => {
                let result = Self::do_enumerate_windows(automation);
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
        // 直接调用现有的捕获逻辑
        Ok(crate::capture::capture_at(x, y))
    }
    
    /// 执行查找操作
    fn do_find_element(
        _automation: &windows::Win32::UI::Accessibility::IUIAutomation,
        window_selector: &str,
        xpath: &str,
        random_range: Option<f32>,
    ) -> anyhow::Result<Vec<ElementInfo>> {
        // 直接调用现有的查找逻辑
        let result = crate::capture::find_all_elements_detailed(
            window_selector,
            xpath,
            random_range.unwrap_or(5.0),
        );
        Ok(result)
    }
    
    /// 执行验证操作
    fn do_validate(
        _automation: &windows::Win32::UI::Accessibility::IUIAutomation,
        window_selector: &str,
        element_xpath: &str,
        hierarchy: &[crate::core::model::HierarchyNode],
    ) -> anyhow::Result<DetailedValidationResult> {
        // 直接调用现有的验证逻辑
        Ok(crate::capture::validate_selector_and_xpath_detailed(
            window_selector,
            element_xpath,
            hierarchy,
        ))
    }
    
    /// 枚举窗口
    fn do_enumerate_windows(
        _automation: &windows::Win32::UI::Accessibility::IUIAutomation,
    ) -> anyhow::Result<Vec<WindowInfo>> {
        // 调用现有的窗口枚举逻辑
        Ok(crate::core::enum_windows::enumerate_windows_fast())
    }
    
    /// 发送捕获请求
    pub fn capture_at(&self, x: i32, y: i32) -> anyhow::Result<CaptureResult> {
        let (response_sender, response_receiver) = mpsc::channel();
        
        if let Some(ref sender) = self.sender {
            sender.send(UiaRequest::CaptureAt {
                x,
                y,
                response: response_sender,
            })?;
            
            response_receiver.recv()?
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
            
            response_receiver.recv()?
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
            
            response_receiver.recv()?
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
