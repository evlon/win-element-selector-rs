// src/api/window.rs
//
// 窗口列表 API

use actix_web::{HttpResponse, Responder};
use log::info;

use super::types::WindowListResponse;

/// POST /api/window/list
/// 列出当前所有可用窗口
pub async fn list_windows() -> impl Responder {
    info!("API: /api/window/list");
    
    // UI Automation 操作需要在 STA 线程中执行
    let windows = tokio::task::spawn_blocking(|| {
        // 在阻塞线程中初始化 COM (STA) - UI Automation 需要
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
    
    // 转换为 API 响应格式
    let response = WindowListResponse {
        windows: windows.into_iter().map(|w| w.into()).collect(),
    };
    
    info!("Found {} windows", response.windows.len());
    HttpResponse::Ok().json(response)
}