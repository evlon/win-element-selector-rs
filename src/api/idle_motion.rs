// src/api/idle_motion.rs
//
// 空闲移动 API - 在指定元素区域内持续随机移动鼠标

use actix_web::{web, HttpResponse, Responder};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use rand::Rng;

use super::types::{Point, Rect, WindowSelector};
use crate::mouse_control;

// ═══════════════════════════════════════════════════════════════════════════════
// 暂停原因枚举
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PauseReason {
    ApiCall,
    HumanMouse,
    HumanKeyboard,
    Manual,
}

// ═══════════════════════════════════════════════════════════════════════════════
// 空闲移动参数
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Deserialize)]
pub struct HumanInterventionConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub pause_on_mouse: bool,
    #[serde(default = "default_true")]
    pub pause_on_keyboard: bool,
    #[serde(default = "default_resume_delay")]
    pub resume_delay: u64,
}

fn default_true() -> bool { true }
fn default_resume_delay() -> u64 { 3000 }

#[derive(Debug, Clone, Deserialize)]
pub struct IdleMotionStartRequest {
    pub window: WindowSelector,
    pub xpath: String,
    #[serde(default = "default_speed")]
    pub speed: String,
    #[serde(default = "default_move_interval")]
    pub move_interval: u64,
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout: u64,
    #[serde(default)]
    pub human_intervention: Option<HumanInterventionConfig>,
}

fn default_speed() -> String { "normal".to_string() }
fn default_move_interval() -> u64 { 800 }
fn default_idle_timeout() -> u64 { 60000 }

#[derive(Debug, Clone, Serialize)]
pub struct IdleMotionStartResponse {
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdleMotionStopResponse {
    pub success: bool,
    pub duration_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdleMotionStatusResponse {
    pub active: bool,
    pub paused: bool,
    pub pause_reason: Option<PauseReason>,
    pub current_rect: Option<Rect>,
    pub running_duration_ms: Option<u64>,
    pub last_activity_ms: Option<u64>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// 空闲移动状态
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct IdleMotionParams {
    pub window_selector: String,
    pub xpath: String,
    pub speed: String,
    pub move_interval: u64,
    pub idle_timeout: u64,
    pub human_intervention: HumanInterventionConfig,
}

pub struct IdleMotionState {
    pub active: bool,
    pub paused: bool,
    pub params: Option<IdleMotionParams>,
    pub cancel_token: Option<CancellationToken>,
    pub current_rect: Option<Rect>,
    
    // 服务端控制标志
    pub server_moving_mouse: bool,
    
    // 时间戳记录
    pub started_at: Option<Instant>,
    pub last_api_call: Option<Instant>,
    pub last_human_activity: Option<Instant>,
    
    // 暂停原因
    pub pause_reason: Option<PauseReason>,
}

impl Default for IdleMotionState {
    fn default() -> Self {
        Self {
            active: false,
            paused: false,
            params: None,
            cancel_token: None,
            current_rect: None,
            server_moving_mouse: false,
            started_at: None,
            last_api_call: None,
            last_human_activity: None,
            pause_reason: None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 全局状态
// ═══════════════════════════════════════════════════════════════════════════════

use once_cell::sync::Lazy;

pub static IDLE_STATE: Lazy<Arc<RwLock<IdleMotionState>>> = Lazy::new(|| {
    Arc::new(RwLock::new(IdleMotionState::default()))
});

// ═══════════════════════════════════════════════════════════════════════════════
// 内部操作函数
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn pause_idle_motion(reason: PauseReason) {
    let mut state = IDLE_STATE.write().await;
    if state.active && !state.paused {
        state.paused = true;
        state.pause_reason = Some(reason);
        info!("Idle motion paused: {:?}", reason);
    }
}

pub async fn resume_idle_motion() {
    let mut state = IDLE_STATE.write().await;
    if state.active && state.paused {
        // 只恢复人工干预导致的暂停
        if matches!(state.pause_reason, Some(PauseReason::HumanMouse) | Some(PauseReason::HumanKeyboard)) {
            state.paused = false;
            state.pause_reason = None;
            info!("Idle motion resumed");
        }
    }
}

pub async fn update_last_api_call() {
    let mut state = IDLE_STATE.write().await;
    state.last_api_call = Some(Instant::now());
}

pub async fn update_last_human_activity() {
    let mut state = IDLE_STATE.write().await;
    state.last_human_activity = Some(Instant::now());
}

pub async fn set_server_moving_mouse(value: bool) {
    let mut state = IDLE_STATE.write().await;
    state.server_moving_mouse = value;
}

// ═══════════════════════════════════════════════════════════════════════════════
// 自动暂停/恢复包装函数
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn with_auto_pause<F, Fut, T>(f: F) -> T
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    // 1. 暂停空闲移动
    pause_idle_motion(PauseReason::ApiCall).await;
    
    // 2. 标记服务端正在移动鼠标
    set_server_moving_mouse(true).await;
    
    // 3. 记录 API 调用时间
    update_last_api_call().await;
    
    // 4. 执行实际操作
    let result = f().await;
    
    // 5. 取消服务端移动标记
    set_server_moving_mouse(false).await;
    
    // 6. 恢复空闲移动
    {
        let mut state = IDLE_STATE.write().await;
        if state.active && state.paused && state.pause_reason == Some(PauseReason::ApiCall) {
            state.paused = false;
            state.pause_reason = None;
            info!("Idle motion auto-resumed after API call");
        }
    }
    
    result
}

// ═══════════════════════════════════════════════════════════════════════════════
// 后台监控任务 (Phase 2)
// ═══════════════════════════════════════════════════════════════════════════════

/// 启动所有后台监控任务
pub fn spawn_background_tasks(cancel_token: CancellationToken) {
    let state = IDLE_STATE.clone();
    
    // 人工鼠标移动检测
    tokio::spawn(human_mouse_monitor(state.clone(), cancel_token.clone()));
    
    // 自动恢复检测
    tokio::spawn(auto_resume_monitor(state.clone(), cancel_token.clone()));
    
    // 空闲超时检测
    tokio::spawn(idle_timeout_monitor(state.clone(), cancel_token.clone()));
    
    // 空闲移动执行任务
    tokio::spawn(idle_motion_task(state.clone(), cancel_token));
    
    info!("Background tasks spawned");
}

/// 检测人工鼠标移动
async fn human_mouse_monitor(state: Arc<RwLock<IdleMotionState>>, cancel_token: CancellationToken) {
    let mut last_mouse_pos = mouse_control::get_cursor_position();
    
    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                info!("Human mouse monitor stopped");
                break;
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                let state_guard = state.read().await;
                
                // 检查是否启用人工干预检测
                if !state_guard.active {
                    continue;
                }
                
                let human_intervention_enabled = state_guard.params.as_ref()
                    .map(|p| p.human_intervention.enabled)
                    .unwrap_or(false);
                
                if !human_intervention_enabled {
                    continue;
                }
                
                // 如果服务端正在移动鼠标，跳过检测
                if state_guard.server_moving_mouse {
                    continue;
                }
                
                let pause_on_mouse = state_guard.params.as_ref()
                    .map(|p| p.human_intervention.pause_on_mouse)
                    .unwrap_or(true);
                
                if !pause_on_mouse {
                    continue;
                }
                
                drop(state_guard);
                
                // 检测鼠标位置变化
                let current_pos = mouse_control::get_cursor_position();
                if current_pos.x != last_mouse_pos.x || current_pos.y != last_mouse_pos.y {
                    // 检测到人工鼠标移动
                    pause_idle_motion(PauseReason::HumanMouse).await;
                    update_last_human_activity().await;
                    info!("Human mouse movement detected, idle motion paused");
                }
                
                last_mouse_pos = current_pos;
            }
        }
    }
}

/// 检测是否应该恢复空闲移动
async fn auto_resume_monitor(state: Arc<RwLock<IdleMotionState>>, cancel_token: CancellationToken) {
    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                info!("Auto resume monitor stopped");
                break;
            }
            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                let state_guard = state.read().await;
                
                if !state_guard.active || !state_guard.paused {
                    continue;
                }
                
                // 只处理人工干预导致的暂停
                if !matches!(state_guard.pause_reason,
                    Some(PauseReason::HumanMouse) |
                    Some(PauseReason::HumanKeyboard)) {
                    continue;
                }
                
                let resume_delay = state_guard.params.as_ref()
                    .map(|p| p.human_intervention.resume_delay)
                    .unwrap_or(3000);
                
                let last_human = state_guard.last_human_activity;
                
                if let Some(last) = last_human {
                    // 用户静止超过 resume_delay，自动恢复
                    if last.elapsed() > Duration::from_millis(resume_delay) {
                        drop(state_guard);
                        
                        // 恢复空闲移动
                        let mut state_write = state.write().await;
                        if state_write.active && state_write.paused {
                            state_write.paused = false;
                            state_write.pause_reason = None;
                            info!("Idle motion auto-resumed after user inactivity");
                        }
                    }
                }
            }
        }
    }
}

/// 检测空闲超时
async fn idle_timeout_monitor(state: Arc<RwLock<IdleMotionState>>, cancel_token: CancellationToken) {
    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                info!("Idle timeout monitor stopped");
                break;
            }
            _ = tokio::time::sleep(Duration::from_secs(5)) => {
                let state_guard = state.read().await;
                
                if !state_guard.active {
                    continue;
                }
                
                let idle_timeout = state_guard.params.as_ref()
                    .map(|p| p.idle_timeout)
                    .unwrap_or(60000);
                
                // 0 表示不超时
                if idle_timeout == 0 {
                    continue;
                }
                
                let last_api = state_guard.last_api_call
                    .or(state_guard.started_at)
                    .unwrap_or(Instant::now());
                
                // 无操作超时，自动停止
                if last_api.elapsed() > Duration::from_millis(idle_timeout) {
                    drop(state_guard);
                    
                    // 停止空闲移动
                    let mut state_write = state.write().await;
                    state_write.active = false;
                    state_write.paused = false;
                    state_write.params = None;
                    state_write.current_rect = None;
                    state_write.started_at = None;
                    state_write.pause_reason = None;
                    
                    if let Some(token) = state_write.cancel_token.take() {
                        token.cancel();
                    }
                    
                    info!("Idle motion stopped due to inactivity timeout");
                }
            }
        }
    }
}

/// 空闲移动执行任务
async fn idle_motion_task(state: Arc<RwLock<IdleMotionState>>, cancel_token: CancellationToken) {
    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                info!("Idle motion task stopped");
                break;
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                let state_guard = state.read().await;
                
                // 检查是否活跃且未暂停
                if !state_guard.active || state_guard.paused {
                    continue;
                }
                
                let move_interval = state_guard.params.as_ref()
                    .map(|p| p.move_interval)
                    .unwrap_or(800);
                
                let current_rect = state_guard.current_rect.clone();
                
                drop(state_guard);
                
                // 等待移动间隔
                tokio::time::sleep(Duration::from_millis(move_interval)).await;
                
                // 再次检查状态（可能在等待期间发生变化）
                let state_guard = state.read().await;
                if !state_guard.active || state_guard.paused || state_guard.server_moving_mouse {
                    continue;
                }
                
                if let Some(rect) = &current_rect {
                    // 计算随机目标点
                    let target = calculate_random_point_in_rect(rect);
                    
                    // 标记服务端正在移动
                    drop(state_guard);
                    set_server_moving_mouse(true).await;
                    
                    // 执行拟人化移动
                    let current_pos = mouse_control::get_cursor_position();
                    let _ = mouse_control::humanized_move(current_pos, target, 400, "bezier");
                    
                    // 取消标记
                    set_server_moving_mouse(false).await;
                    
                    info!("Idle motion moved to ({}, {})", target.x, target.y);
                }
            }
        }
    }
}

/// 在矩形区域内计算随机点
fn calculate_random_point_in_rect(rect: &Rect) -> Point {
    let mut rng = rand::thread_rng();
    
    let x = rng.gen_range(rect.x..rect.x + rect.width);
    let y = rng.gen_range(rect.y..rect.y + rect.height);
    
    Point::new(x, y)
}

// ═══════════════════════════════════════════════════════════════════════════════
// API 接口
// ═══════════════════════════════════════════════════════════════════════════════

/// 构建窗口选择器字符串
fn build_window_selector(selector: &WindowSelector) -> String {
    let mut predicates: Vec<String> = Vec::new();
    
    if let Some(ref title) = selector.title {
        predicates.push(format!("@Name='{}'", title));
    }
    if let Some(ref class_name) = selector.class_name {
        predicates.push(format!("@ClassName='{}'", class_name));
    }
    if let Some(ref process_name) = selector.process_name {
        predicates.push(format!("@ProcessName='{}'", process_name));
    }
    
    if predicates.is_empty() {
        "Window".to_string()
    } else {
        format!("Window[{}]", predicates.join(" and "))
    }
}

/// POST /api/mouse/idle/start
pub async fn start_idle_motion(body: web::Json<IdleMotionStartRequest>) -> impl Responder {
    let request = body.into_inner();
    
    info!(
        "API: /api/mouse/idle/start window='{}' xpath='{}'",
        request.window.title.as_deref().unwrap_or(""),
        request.xpath
    );
    
    // 检查当前状态
    let mut state = IDLE_STATE.write().await;
    
    if state.active {
        return HttpResponse::Ok().json(IdleMotionStartResponse {
            success: false,
            error: Some("空闲移动已在运行".to_string()),
        });
    }
    
    // 构建窗口选择器
    let window_selector = build_window_selector(&request.window);
    let xpath = request.xpath.clone();
    
    // 获取元素区域
    let window_selector_clone = window_selector.clone();
    let xpath_clone = xpath.clone();
    let rect_result = tokio::task::spawn_blocking(move || {
        #[cfg(target_os = "windows")]
        {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
            unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            }
        }
        
        crate::capture::validate_selector_and_xpath_detailed(&window_selector_clone, &xpath_clone)
    })
    .await;
    
    match rect_result {
        Ok(detailed_result) => {
            use crate::model::ValidationResult;
            
            match &detailed_result.overall {
                ValidationResult::Found { first_rect, .. } => {
                    if let Some(rect) = first_rect {
                        let api_rect: Rect = rect.clone().into();
                        
                        // 创建取消令牌
                        let cancel_token = CancellationToken::new();
                        
                        // 设置状态
                        state.active = true;
                        state.paused = false;
                        state.current_rect = Some(api_rect.clone());
                        state.started_at = Some(Instant::now());
                        state.last_api_call = Some(Instant::now());
                        state.params = Some(IdleMotionParams {
                            window_selector,
                            xpath: request.xpath,
                            speed: request.speed,
                            move_interval: request.move_interval,
                            idle_timeout: request.idle_timeout,
                            human_intervention: request.human_intervention.unwrap_or(HumanInterventionConfig {
                                enabled: true,
                                pause_on_mouse: true,
                                pause_on_keyboard: true,
                                resume_delay: 3000,
                            }),
                        });
                        state.cancel_token = Some(cancel_token.clone());
                        state.pause_reason = None;
                        
                        drop(state);
                        
                        // 启动后台任务
                        spawn_background_tasks(cancel_token);
                        
                        info!("Idle motion started successfully");
                        
                        HttpResponse::Ok().json(IdleMotionStartResponse {
                            success: true,
                            error: None,
                        })
                    } else {
                        HttpResponse::Ok().json(IdleMotionStartResponse {
                            success: false,
                            error: Some("元素坐标获取失败".to_string()),
                        })
                    }
                }
                ValidationResult::NotFound => {
                    HttpResponse::Ok().json(IdleMotionStartResponse {
                        success: false,
                        error: Some(format!("未找到匹配元素 (耗时 {}ms)", detailed_result.total_duration_ms)),
                    })
                }
                ValidationResult::Error(e) => {
                    HttpResponse::Ok().json(IdleMotionStartResponse {
                        success: false,
                        error: Some(e.clone()),
                    })
                }
                _ => {
                    HttpResponse::Ok().json(IdleMotionStartResponse {
                        success: false,
                        error: Some("校验状态未知".to_string()),
                    })
                }
            }
        }
        Err(e) => {
            warn!("Spawn blocking error: {}", e);
            HttpResponse::InternalServerError().json(IdleMotionStartResponse {
                success: false,
                error: Some(format!("内部错误: {}", e)),
            })
        }
    }
}

/// POST /api/mouse/idle/stop
pub async fn stop_idle_motion() -> impl Responder {
    info!("API: /api/mouse/idle/stop");
    
    let mut state = IDLE_STATE.write().await;
    
    if !state.active {
        return HttpResponse::Ok().json(IdleMotionStopResponse {
            success: false,
            duration_ms: 0,
            error: Some("空闲移动未启动".to_string()),
        });
    }
    
    // 取消后台任务
    if let Some(token) = state.cancel_token.take() {
        token.cancel();
    }
    
    let duration_ms = state.started_at
        .map(|t| t.elapsed().as_millis() as u64)
        .unwrap_or(0);
    
    // 重置状态
    state.active = false;
    state.paused = false;
    state.params = None;
    state.current_rect = None;
    state.started_at = None;
    state.pause_reason = None;
    
    info!("Idle motion stopped, duration={}ms", duration_ms);
    
    HttpResponse::Ok().json(IdleMotionStopResponse {
        success: true,
        duration_ms,
        error: None,
    })
}

/// GET /api/mouse/idle/status
pub async fn get_idle_motion_status() -> impl Responder {
    let state = IDLE_STATE.read().await;
    
    let running_duration_ms = state.started_at
        .map(|t| t.elapsed().as_millis() as u64);
    
    let last_activity_ms = state.last_api_call
        .or(state.last_human_activity)
        .map(|t| t.elapsed().as_millis() as u64);
    
    HttpResponse::Ok().json(IdleMotionStatusResponse {
        active: state.active,
        paused: state.paused,
        pause_reason: state.pause_reason,
        current_rect: state.current_rect.clone(),
        running_duration_ms,
        last_activity_ms,
    })
}