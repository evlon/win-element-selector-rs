// src/api/idle_motion.rs
//
// 空闲移动 API - 在指定元素区域内持续随机移动鼠标

use actix_web::{HttpResponse, Responder};
use log::info;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use super::types::{Rect, WindowSelector};

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
        // 只恢复人工干预导致的暂停，API 调用暂停会在 with_auto_pause 中恢复
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
// API 接口
// ═══════════════════════════════════════════════════════════════════════════════

/// POST /api/mouse/idle/start
pub async fn start_idle_motion() -> impl Responder {
    info!("API: /api/mouse/idle/start - Not yet fully implemented");
    
    // 当前返回成功，实际后台任务尚未启动
    HttpResponse::Ok().json(IdleMotionStartResponse {
        success: true,
        error: Some("空闲移动功能正在开发中，API 调用自动暂停已就绪".to_string()),
    })
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