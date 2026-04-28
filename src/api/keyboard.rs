// src/api/keyboard.rs
//
// 键盘操作 API - 拟人化打字

use actix_web::{web, HttpResponse, Responder};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use rand::Rng;

#[cfg(target_os = "windows")]
use windows::Win32::{
    UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
        KEYEVENTF_UNICODE, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    },
};

// ═══════════════════════════════════════════════════════════════════════════════
// 请求/响应类型
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Deserialize)]
pub struct CharDelayConfig {
    #[serde(default = "default_min_delay")]
    pub min: u64,
    #[serde(default = "default_max_delay")]
    pub max: u64,
}

fn default_min_delay() -> u64 { 50 }
fn default_max_delay() -> u64 { 150 }

#[derive(Debug, Clone, Deserialize)]
pub struct TypeRequest {
    pub text: String,
    #[serde(default)]
    pub char_delay: Option<CharDelayConfig>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TypeResponse {
    pub success: bool,
    pub chars_typed: u32,
    pub duration_ms: u64,
    pub error: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// 键盘输入实现
// ═══════════════════════════════════════════════════════════════════════════════

/// 发送单个字符的键盘输入
#[cfg(target_os = "windows")]
fn send_unicode_char(ch: char) {
    unsafe {
        // 按下
        let key_down = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: ch as u16,
                    dwFlags: KEYEVENTF_UNICODE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        
        // 释放
        let key_up = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: ch as u16,
                    dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        
        let inputs = [key_down, key_up];
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

#[cfg(not(target_os = "windows"))]
fn send_unicode_char(_ch: char) {
    // Non-Windows stub
}

/// 拟人化打字 - 每个字符随机延迟
pub fn humanized_type(text: &str, min_delay: u64, max_delay: u64) -> anyhow::Result<(u32, u64)> {
    let start = Instant::now();
    let mut rng = rand::thread_rng();
    let mut chars_typed = 0u32;
    
    for ch in text.chars() {
        // 发送字符
        send_unicode_char(ch);
        chars_typed += 1;
        
        // 随机延迟
        let delay = rng.gen_range(min_delay..max_delay + 1);
        std::thread::sleep(Duration::from_millis(delay));
    }
    
    let duration_ms = start.elapsed().as_millis() as u64;
    Ok((chars_typed, duration_ms))
}

// ═══════════════════════════════════════════════════════════════════════════════
// API 接口
// ═══════════════════════════════════════════════════════════════════════════════

/// POST /api/keyboard/type
pub async fn type_text(body: web::Json<TypeRequest>) -> impl Responder {
    let request = body.into_inner();
    
    info!(
        "API: /api/keyboard/type text_len={} chars",
        request.text.chars().count()
    );
    
    if request.text.is_empty() {
        return HttpResponse::Ok().json(TypeResponse {
            success: true,
            chars_typed: 0,
            duration_ms: 0,
            error: None,
        });
    }
    
    let char_delay = request.char_delay.unwrap_or(CharDelayConfig {
        min: default_min_delay(),
        max: default_max_delay(),
    });
    
    // 在阻塞线程中执行打字
    let text = request.text.clone();
    let min_delay = char_delay.min;
    let max_delay = char_delay.max;
    
    let result = tokio::task::spawn_blocking(move || {
        humanized_type(&text, min_delay, max_delay)
    })
    .await;
    
    match result {
        Ok(Ok((chars_typed, duration_ms))) => {
            info!("Type completed: {} chars in {}ms", chars_typed, duration_ms);
            HttpResponse::Ok().json(TypeResponse {
                success: true,
                chars_typed,
                duration_ms,
                error: None,
            })
        }
        Ok(Err(e)) => {
            warn!("Type failed: {}", e);
            HttpResponse::Ok().json(TypeResponse {
                success: false,
                chars_typed: 0,
                duration_ms: 0,
                error: Some(e.to_string()),
            })
        }
        Err(e) => {
            warn!("Spawn blocking error: {}", e);
            HttpResponse::InternalServerError().json(TypeResponse {
                success: false,
                chars_typed: 0,
                duration_ms: 0,
                error: Some(format!("内部错误: {}", e)),
            })
        }
    }
}