// src/bin/server.rs
//
// HTTP 服务入口 - 提供 UI Automation API 接口
// 使用 actix-web 框架，独立于 GUI 应用运行

use actix_web::{web, App, HttpServer, HttpResponse, Responder};
use log::info;

// 从库模块导入
use element_selector::api::{element, mouse, window};

/// 健康检查接口
async fn health_check() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({
        "status": "ok",
        "version": "1.0.0",
        "service": "element-selector-server"
    }))
}

/// 主入口
#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    // 初始化日志
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .init();
    
    info!("element-selector-server starting on port 8080");
    
    // Windows: COM 必须在主线程初始化 (STA)
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                .ok()
                .expect("CoInitializeEx failed");
        }
        info!("COM initialized (STA)");
    }
    
    // 配置 HTTP 服务
    HttpServer::new(|| {
        App::new()
            // 健康检查
            .route("/api/health", web::get().to(health_check))
            // 窗口列表
            .route("/api/window/list", web::post().to(window::list_windows))
            // 元素查找
            .route("/api/element", web::get().to(element::get_element))
            // 鼠标移动
            .route("/api/mouse/move", web::post().to(mouse::move_mouse))
            // 鼠标点击
            .route("/api/mouse/click", web::post().to(mouse::click_mouse))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await?;
    
    info!("element-selector-server stopped");
    Ok(())
}