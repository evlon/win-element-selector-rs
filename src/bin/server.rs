// src/bin/server.rs
//
// HTTP 服务入口 - 提供 UI Automation API 接口
// 使用 actix-web 框架，独立于 GUI 应用运行

use actix_web::{web, App, HttpServer, HttpResponse, Responder};
use clap::Parser;
use log::{info, LevelFilter};

// 从库模块导入
use element_selector::api::{element, mouse, window, idle_motion, keyboard};

/// Element Selector Server - Windows UI Automation HTTP 服务
/// 
/// 提供元素查找、鼠标控制、窗口列表等 API 接口
#[derive(Parser, Debug)]
#[command(name = "element-selector-server")]
#[command(version = "1.0.0")]
#[command(about = "Windows UI Automation HTTP 服务", long_about = None)]
struct Args {
    /// 绑定的 IP 地址
    #[arg(short, long, default_value = "127.0.0.1")]
    bind: String,

    /// 监听端口
    #[arg(short, long, default_value_t = 8080)]
    port: u16,

    /// 详细日志级别 (可多次使用: -v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

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
    // 解析命令行参数
    let args = Args::parse();

    // 根据详细级别设置日志级别
    let log_level = match args.verbose {
        0 => LevelFilter::Info,
        1 => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    };

    // 初始化日志
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .filter_level(log_level)
    .init();

    let bind_addr = format!("{}:{}", args.bind, args.port);
    info!("element-selector-server starting on {}", bind_addr);
    
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
            // 窗口 API
            .route("/api/window/list", web::post().to(window::list_windows))
            .route("/api/window/activate", web::post().to(window::activate_window))
            .route("/api/window/focus-element", web::post().to(window::focus_element))
            // 元素查找
            .route("/api/element", web::get().to(element::get_element))
            .route("/api/element/all", web::get().to(element::get_all_elements))
            // 鼠标移动
            .route("/api/mouse/move", web::post().to(mouse::move_mouse))
            // 鼠标点击
            .route("/api/mouse/click", web::post().to(mouse::click_mouse))
            // 空闲移动 API
            .route("/api/mouse/idle/start", web::post().to(idle_motion::start_idle_motion))
            .route("/api/mouse/idle/stop", web::post().to(idle_motion::stop_idle_motion))
            .route("/api/mouse/idle/status", web::get().to(idle_motion::get_idle_motion_status))
            // 键盘 API
            .route("/api/keyboard/type", web::post().to(keyboard::type_text))
    })
    .bind(&bind_addr)?
    .run()
    .await?;
    
    info!("element-selector-server stopped");
    Ok(())
}