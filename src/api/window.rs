// src/api/window.rs
//
// 窗口 API - 窗口列表和激活

use actix_web::{HttpResponse, Responder, web};
use log::info;
use serde::Deserialize;

use super::types::WindowListResponse;

/// POST /api/window/list
/// 列出当前所有可用窗口
pub async fn list_windows() -> impl Responder {
    info!("API: /api/window/list");
    
    let windows = tokio::task::spawn_blocking(|| {
        #[cfg(target_os = "windows")]
        {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
            unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            }
        }
        
        let result = super::super::capture::list_windows();
        result
    })
    .await
    .unwrap_or_default();
    
    let response = WindowListResponse {
        windows: windows.into_iter().map(|w| w.into()).collect(),
    };
    
    info!("Found {} windows", response.windows.len());
    HttpResponse::Ok().json(response)
}

// ═══════════════════════════════════════════════════════════════════════════════
// 激活窗口 API
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
pub struct ActivateWindowRequest {
    /// 窗口选择器 XPath
    /// 例如: "Window[@Name='微信' and @ClassName='mmui::MainWindow']"
    #[serde(rename = "windowSelector")]
    pub window_selector: String,
}

#[derive(Debug, serde::Serialize)]
pub struct ActivateWindowResponse {
    pub success: bool,
    #[serde(rename = "windowSelector")]
    pub window_selector: String,
    pub error: Option<String>,
}

/// POST /api/window/activate
/// 激活指定窗口（使其成为前台窗口）
pub async fn activate_window(req: web::Json<ActivateWindowRequest>) -> impl Responder {
    info!("API: /api/window/activate - selector: {}", req.window_selector);
    
    let window_selector = req.window_selector.clone();
    
    let success = tokio::task::spawn_blocking(move || {
        #[cfg(target_os = "windows")]
        {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
            unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            }
        }
        
        super::super::core::uia::windows_impl::activate_window_by_selector(&window_selector)
    })
    .await
    .unwrap_or(false);
    
    let response = ActivateWindowResponse {
        success,
        window_selector: req.window_selector.clone(),
        error: if success { None } else { Some("窗口未找到或激活失败".to_string()) },
    };
    
    info!("Activate result: {}", success);
    HttpResponse::Ok().json(response)
}

/// POST /api/window/focus-element
/// 激活窗口并使指定元素获得焦点
#[derive(Debug, Deserialize)]
pub struct FocusElementRequest {
    #[serde(rename = "windowSelector")]
    pub window_selector: String,
    pub xpath: String,
}

#[derive(Debug, serde::Serialize)]
pub struct FocusElementResponse {
    pub success: bool,
    pub error: Option<String>,
}

pub async fn focus_element(req: web::Json<FocusElementRequest>) -> impl Responder {
    info!("API: /api/window/focus-element - window: {}, xpath: {}", req.window_selector, req.xpath);
    
    let window_selector = req.window_selector.clone();
    let xpath = req.xpath.clone();
    
    let success = tokio::task::spawn_blocking(move || {
        #[cfg(target_os = "windows")]
        {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
            unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            }
        }
        
        super::super::core::uia::windows_impl::activate_and_focus_element(&window_selector, &xpath)
    })
    .await
    .unwrap_or(false);
    
    HttpResponse::Ok().json(FocusElementResponse {
        success,
        error: if success { None } else { Some("窗口或元素未找到".to_string()) },
    })
}