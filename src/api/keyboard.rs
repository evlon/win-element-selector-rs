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
        VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN,
        VK_RETURN, VK_TAB, VK_ESCAPE, VK_HOME, VK_END,
        VK_DELETE, VK_BACK, VK_INSERT, VK_PRIOR, VK_NEXT,
        VK_LEFT, VK_RIGHT, VK_UP, VK_DOWN,
        VK_F1, VK_F2, VK_F3, VK_F4, VK_F5, VK_F6,
        VK_F7, VK_F8, VK_F9, VK_F10, VK_F11, VK_F12,
    },
};

// ═══════════════════════════════════════════════════════════════════════════════
// 快捷键请求类型
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Deserialize)]
pub struct ShortcutRequest {
    /// 快捷键组合，如 "Ctrl+C", "Alt+F4", "Ctrl+Shift+S"
    pub keys: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KeyRequest {
    /// 单个按键名称，如 "Enter", "Tab", "Escape", "Home", "End"
    pub key: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct KeyResponse {
    pub success: bool,
    pub error: Option<String>,
}

/// 解析按键名称到虚拟键码
fn parse_key_name(key_name: &str) -> u16 {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::*;
        match key_name.to_lowercase().as_str() {
            "enter" | "return" => VK_RETURN.0,
            "tab" => VK_TAB.0,
            "escape" | "esc" => VK_ESCAPE.0,
            "home" => VK_HOME.0,
            "end" => VK_END.0,
            "delete" | "del" => VK_DELETE.0,
            "backspace" | "back" => VK_BACK.0,
            "insert" | "ins" => VK_INSERT.0,
            "pageup" | "pgup" => VK_PRIOR.0,
            "pagedown" | "pgdn" => VK_NEXT.0,
            "left" => VK_LEFT.0,
            "right" => VK_RIGHT.0,
            "up" => VK_UP.0,
            "down" => VK_DOWN.0,
            "f1" => VK_F1.0,
            "f2" => VK_F2.0,
            "f3" => VK_F3.0,
            "f4" => VK_F4.0,
            "f5" => VK_F5.0,
            "f6" => VK_F6.0,
            "f7" => VK_F7.0,
            "f8" => VK_F8.0,
            "f9" => VK_F9.0,
            "f10" => VK_F10.0,
            "f11" => VK_F11.0,
            "f12" => VK_F12.0,
            // 单字符按键
            c if c.len() == 1 => {
                let ch = c.chars().next().unwrap();
                if ch.is_ascii_alphabetic() {
                    (ch.to_ascii_uppercase() as u16) - ('A' as u16) + 0x41
                } else if ch.is_ascii_digit() {
                    ch as u16
                } else {
                    0
                }
            },
            _ => 0,
        }
    }
    #[cfg(not(target_os = "windows"))]
    { 0 }
}

/// 解析快捷键字符串并执行
#[cfg(target_os = "windows")]
fn execute_shortcut(keys_str: &str) -> anyhow::Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
        KEYEVENTF_KEYUP, VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN,
    };    
    
    // 解析按键组合
    let parts: Vec<&str> = keys_str.split('+').collect();
    if parts.is_empty() {
        return Err(anyhow::anyhow!("Invalid shortcut format"));
    }
    
    // 获取修饰键和目标键
    let mut modifiers: Vec<u16> = Vec::new();
    let target_key = parts.last().unwrap();
    
    for part in &parts[..parts.len() - 1] {
        let mod_key = match part.trim().to_lowercase().as_str() {
            "ctrl" | "control" => VK_CONTROL.0,
            "shift" => VK_SHIFT.0,
            "alt" | "menu" => VK_MENU.0,
            "win" | "windows" => VK_LWIN.0,
            _ => continue,
        };        modifiers.push(mod_key);
    }
    
    // 获取目标键的虚拟键码
    let target_vk = parse_key_name(target_key.trim());
    
    // 构建输入序列：按下修饰键 -> 按下目标键 -> 释放目标键 -> 释放修饰键
    let mut inputs: Vec<INPUT> = Vec::new();
    
    // 按下修饰键
    for &mod_key in &modifiers {
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(mod_key),
                    wScan: 0,
                    dwFlags: 0, // 按下
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
    }
    
    // 按下目标键
    inputs.push(INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(target_vk),
                wScan: 0,
                dwFlags: 0, // 按下
                time: 0,
                dwExtraInfo: 0,
            },
        },
    });
    
    // 短暂等待
    std::thread::sleep(std::time::Duration::from_millis(50));
    
    // 释放目标键
    inputs.push(INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(target_vk),
                wScan: 0,
                dwFlags: KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    });
    
    // 释放修饰键
    for &mod_key in &modifiers {
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(mod_key),
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
    }
    
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
    
    Ok(())
}
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
    #[serde(default, rename = "charDelay")]
    pub char_delay: Option<CharDelayConfig>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TypeResponse {
    pub success: bool,
    #[serde(rename = "charsTyped")]
    pub chars_typed: u32,
    #[serde(rename = "durationMs")]
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

/// POST /api/keyboard/shortcut
/// 执行快捷键组合
pub async fn execute_shortcut_api(body: web::Json<ShortcutRequest>) -> impl Responder {
    let request = body.into_inner();
    
    info!("API: /api/keyboard/shortcut keys='{}'", request.keys);
    
    let keys = request.keys.clone();
    
    let result = tokio::task::spawn_blocking(move || {
        execute_shortcut(&keys)
    })
    .await;
    
    match result {
        Ok(Ok(())) => {
            info!("Shortcut executed successfully");
            HttpResponse::Ok().json(KeyResponse {
                success: true,
                error: None,
            })
        }
        Ok(Err(e)) => {
            warn!("Shortcut failed: {}", e);
            HttpResponse::Ok().json(KeyResponse {
                success: false,
                error: Some(e.to_string()),
            })
        }
        Err(e) => {
            warn!("Spawn blocking error: {}", e);
            HttpResponse::InternalServerError().json(KeyResponse {
                success: false,
                error: Some(format!("内部错误: {}", e)),
            })
        }
    }
}

/// POST /api/keyboard/key
/// 执行单个按键
pub async fn execute_key_api(body: web::Json<KeyRequest>) -> impl Responder {
    let request = body.into_inner();
    
    info!("API: /api/keyboard/key key='{}'", request.key);
    
    let key = request.key.clone();
    
    let result = tokio::task::spawn_blocking(move || {
        execute_key(&key)
    })
    .await;
    
    match result {
        Ok(Ok(())) => {
            info!("Key executed successfully");
            HttpResponse::Ok().json(KeyResponse {
                success: true,
                error: None,
            })
        }
        Ok(Err(e)) => {
            warn!("Key failed: {}", e);
            HttpResponse::Ok().json(KeyResponse {
                success: false,
                error: Some(e.to_string()),
            })
        }
        Err(e) => {
            warn!("Spawn blocking error: {}", e);
            HttpResponse::InternalServerError().json(KeyResponse {
                success: false,
                error: Some(format!("内部错误: {}", e)),
            })
        }
    }
}

/// 执行单个按键
#[cfg(target_os = "windows")]
fn execute_key(key_name: &str) -> anyhow::Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
        KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };    
    
    let vk = parse_key_name(key_name);
    if vk == 0 {
        return Err(anyhow::anyhow!("Unknown key: {}", key_name));
    }
    
    // 按下
    let key_down = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: 0,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };    
    
    // 短暂等待
    std::thread::sleep(std::time::Duration::from_millis(50));
    
    // 释放
    let key_up = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };    
    
    let inputs = [key_down, key_up];
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
    
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn execute_key(_key_name: &str) -> anyhow::Result<()> {
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// 单元测试
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    
    /// 测试 CharDelayConfig 默认值
    #[test]
    fn test_char_delay_defaults() {
        assert_eq!(default_min_delay(), 50);
        assert_eq!(default_max_delay(), 150);
    }
    
    /// 测试 TypeRequest 反序列化
    #[test]
    fn test_type_request_deserialization() {
        let json = r#"{"text":"Hello"}"#;
        let request: TypeRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.text, "Hello");
        assert!(request.char_delay.is_none());
    }
    
    /// 测试 TypeRequest 带延迟参数
    #[test]
    fn test_type_request_with_delay() {
        let json = r#"{"text":"Test","charDelay":{"min":30,"max":80}}"#;
        let request: TypeRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.text, "Test");
        assert!(request.char_delay.is_some());
        let delay = request.char_delay.unwrap();
        assert_eq!(delay.min, 30);
        assert_eq!(delay.max, 80);
    }
    
    /// 测试 TypeResponse 序列化
    #[test]
    fn test_type_response_serialization() {
        let response = TypeResponse {
            success: true,
            chars_typed: 10,
            duration_ms: 500,
            error: None,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("success"));
        assert!(json.contains("charsTyped"));
        assert!(json.contains("durationMs"));
    }
    
    /// 测试空文本处理
    #[test]
    fn test_empty_text_handling() {
        let request = TypeRequest {
            text: String::new(),
            char_delay: None,
        };
        assert!(request.text.is_empty());
    }
    
    /// 测试 Unicode 文本
    #[test]
    fn test_unicode_text() {
        let request = TypeRequest {
            text: "中文测试🎉".to_string(),
            char_delay: None,
        };
        assert_eq!(request.text.chars().count(), 5);
    }
}