// tests/common/mod.rs
//
// 集成测试共享基础设施
// 提供: 动态端口 HTTP 服务器启动/停止、缓存预注入、HTTP 客户端

use actix_web::{web, App, HttpServer, HttpResponse, Responder};
use element_selector::api::{element, window};
use reqwest::Client;
use std::net::TcpListener;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use uiautomation::core::UIElement;

/// 全局测试互斥锁：防止多个集成测试并发操作同一全局缓存/服务器
static TEST_MUTEX: Mutex<()> = Mutex::new(());

/// 测试端口计数器（每次递增，避免端口冲突）
static TEST_PORT: AtomicU16 = AtomicU16::new(19800);

/// 测试 HTTP 客户端 (reqwest, 超时 10s)
pub struct TestClient {
    pub base_url: String,
    pub client: Client,
}

impl TestClient {
    pub fn new(port: u16) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to create reqwest client");
        Self {
            base_url: format!("http://127.0.0.1:{port}"),
            client,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }
}

/// 启动测试 HTTP 服务器，返回 (port, server_handle, shutdown_tx)。
///
/// 服务器在随机端口上启动，返回后可发送 HTTP 请求。
pub async fn start_test_server() -> (u16, actix_web::dev::ServerHandle) {
    let port = find_available_port();
    let server = build_test_server(port);
    let handle = server.handle();
    tokio::spawn(server);
    // Wait for server to be ready
    wait_for_server(port).await;
    (port, handle)
}

fn find_available_port() -> u16 {
    // Try the atomic counter first
    for _ in 0..100 {
        let port = TEST_PORT.fetch_add(1, Ordering::SeqCst);
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
    panic!("Could not find an available port");
}

fn build_test_server(port: u16) -> actix_web::dev::Server {
    // We MUST NOT init UIA context or narrator in tests (it's already done in main).
    // Instead we use the lazy-initialized cache directly.

    HttpServer::new(|| {
        App::new()
            // 健康检查
            .route("/api/health", web::get().to(health_check))
            // 窗口 API
            .route("/api/window/list", web::post().to(window::list_windows))
            .route("/api/window/activate", web::post().to(window::activate_window))
            .route("/api/window/exists", web::post().to(window::exists_window))
            .route("/api/window/focus-element", web::post().to(window::focus_element))
            // 元素查找
            .route("/api/element", web::get().to(element::get_element))
            .route("/api/element", web::post().to(element::get_element))
            .route("/api/element/all", web::get().to(element::get_all_elements))
            .route("/api/element/all", web::post().to(element::get_all_elements))
            // 元素可视区域
            .route("/api/element/visibility", web::post().to(element::get_element_visibility))
            // 元素高亮闪烁
            .route("/api/element/flash", web::post().to(element::flash_element))
            // 元素 Inspect
            .route("/api/element/inspect", web::post().to(element::inspect_element))
            // 元素导航
            .route("/api/element/navigate", web::post().to(element::navigate_element))
            // 从已知元素查找
            .route("/api/element/find-from", web::post().to(element::find_from_element))
            // 通过 runtimeId 刷新
            .route("/api/element/refresh", web::post().to(element::refresh_by_runtime_id))
            // 元素缓存控制
            .route("/api/element/cache/config", web::put().to(element::set_cache_config))
            .route("/api/element/cache/stats", web::get().to(element::get_cache_stats))
            .route("/api/element/cache/clear", web::post().to(element::clear_element_cache))
            // XPath 缓存
            .route("/api/xpath-cache/stats", web::get().to(element::get_xpath_cache_stats))
            .route("/api/xpath-cache/clear", web::post().to(element::clear_xpath_cache_handler))
    })
    .keep_alive(Duration::from_secs(5))
    .client_request_timeout(Duration::from_secs(10))
    .client_disconnect_timeout(Duration::from_secs(5))
    .bind(("127.0.0.1", port))
    .expect("Failed to bind test server")
    .run()
}

/// 健康检查接口（与 server.rs 一致）
async fn health_check() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({
        "status": "ok",
        "version": "1.0.0",
        "service": "element-selector-server"
    }))
}

async fn wait_for_server(port: u16) {
    let url = format!("http://127.0.0.1:{port}/api/health");
    for _ in 0..50 {
        if reqwest::get(&url).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("Test server did not start in time");
}

// ── 缓存操作辅助 ───────────────────────────────────────────────────────

/// 获取桌面根元素用于测试（跳过无 UIA 环境）
pub fn get_desktop_element() -> Option<UIElement> {
    uiautomation::UIAutomation::new()
        .ok()
        .and_then(|automation| automation.get_root_element().ok())
}

/// 预先向缓存注入元素
pub fn seed_cache(runtime_id: &str, element: &UIElement) {
    element_selector::core::element_cache::cache_element(
        runtime_id.to_string(),
        element.clone(),
    );
}

/// 清空缓存
pub fn clear_cache() {
    element_selector::core::element_cache::clear_cache();
}

/// 设置全局 TTL
pub fn set_cache_ttl(ttl_ms: Option<u64>) {
    element_selector::core::element_cache::set_default_ttl(
        ttl_ms.map(Duration::from_millis),
    );
}

/// 获取测试互斥锁（用于序列化集成测试）
pub fn acquire_test_lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner())
}

/// 获取一个可用的桌面根元素及其 RuntimeId
pub fn get_desktop_with_rid() -> Option<(UIElement, String)> {
    let elem = get_desktop_element()?;
    let rid = elem.get_runtime_id().ok()?;
    let rid_str = rid.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
    Some((elem, rid_str))
}

/// 通过 HTTP 获取可用的窗口选择器（用于 XPath 搜索测试）
pub async fn get_first_available_window(client: &TestClient) -> Option<String> {
    let resp = client.client
        .post(&client.url("/api/window/list"))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: serde_json::Value = resp.json().await.ok()?;
    let windows = body.get("windows").and_then(|w| w.as_array())?;
    if windows.is_empty() {
        return None;
    }
    // 优先找 Program Manager (桌面) 或 explorer
    for w in windows {
        let name = w.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let class = w.get("className").and_then(|c| c.as_str()).unwrap_or("");
        // Program Manager 是桌面窗口
        if class == "Progman" || name.contains("Program Manager") {
            return Some(format!("Window[@ClassName='{}']", class));
        }
    }
    // fallback: 用第一个窗口的类名
    let first = &windows[0];
    let class = first.get("className").and_then(|c| c.as_str()).unwrap_or("");
    if !class.is_empty() {
        return Some(format!("Window[@ClassName='{}']", class));
    }
    None
}

/// 通过 XPath 在窗口内搜索元素，返回 (found, element_info)
pub async fn search_element(
    client: &TestClient,
    window_selector: &str,
    xpath: &str,
) -> Result<serde_json::Value, reqwest::Error> {
    let resp = client.client
        .post(&client.url("/api/element"))
        .json(&serde_json::json!({
            "window": window_selector,
            "element": xpath,
        }))
        .send()
        .await?;
    resp.json().await
}
