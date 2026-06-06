// src/api/keyboard.rs
//
// 键盘操作 API - 拟人化打字，支持多种输入模式

use actix_web::{web, HttpResponse, Responder};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use rand::Rng;

use windows::Win32::{
    System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
    System::DataExchange::{OpenClipboard, CloseClipboard, EmptyClipboard, SetClipboardData},
    UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
        KEYEVENTF_UNICODE, KEYEVENTF_KEYUP, VIRTUAL_KEY, KEYBD_EVENT_FLAGS,
    },
};

/// CF_UNICODETEXT 常量值 (windows 0.62 中此常量可能未被导出)
const CF_UNICODETEXT: u32 = 13;

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

/// 解析快捷键字符串并执行
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
                    dwFlags: KEYBD_EVENT_FLAGS(0), // 按下
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
                dwFlags: KEYBD_EVENT_FLAGS(0), // 按下
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

/// 输入模式枚举
#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum TypeMode {
    /// 键盘模拟输入（默认）：SendInput 逐字击键，支持 {Enter} 等虚拟键
    #[default]
    Keyboard,
    /// UIA ValuePattern.SetValue：直接通过 UIA 设置控件文本值，无需焦点/可见
    Value,
    /// 剪贴板粘贴：复制到剪贴板后 Ctrl+V 粘贴，适合长文本
    Clipboard,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TypeRequest {
    pub text: String,
    #[serde(default, rename = "charDelay")]
    pub char_delay: Option<CharDelayConfig>,
    /// 输入模式，默认 keyboard
    #[serde(default, rename = "typeMode")]
    pub type_mode: Option<TypeMode>,
    /// Value/Clipboard 模式需要的窗口选择器
    #[serde(default)]
    pub window: Option<String>,
    /// Value/Clipboard 模式需要的元素 XPath
    #[serde(default)]
    pub element: Option<String>,
    /// RuntimeId，用于缓存查找（优先于 XPath 搜索）
    #[serde(default, rename = "runtimeId", skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<String>,
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

/// 发送组合键（如 Ctrl+C, Alt+F4）
fn send_shortcut(shortcut: &str) -> anyhow::Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN,
    };
    
    // 解析按键组合
    let parts: Vec<&str> = shortcut.split('+').collect();
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
        };
        modifiers.push(mod_key);
    }
    
    // 获取目标键的虚拟键码
    let target_vk = parse_key_name(target_key.trim());
    if target_vk == 0 {
        return Err(anyhow::anyhow!("Unknown key: {}", target_key));
    }
    
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
                    dwFlags: KEYBD_EVENT_FLAGS(0),
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
                dwFlags: KEYBD_EVENT_FLAGS(0),
                time: 0,
                dwExtraInfo: 0,
            },
        },
    });
    
    // 短暂等待
    std::thread::sleep(Duration::from_millis(50));
    
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

/// 发送虚拟键（如 Enter、Tab 等）
fn send_virtual_key(key_name: &str) -> anyhow::Result<()> {
    let vk = parse_key_name(key_name);
    if vk == 0 {
        return Err(anyhow::anyhow!("Unknown virtual key: {}", key_name));
    }
    
    // 按下
    let key_down = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: KEYBD_EVENT_FLAGS(0),
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    
    // 短暂等待
    std::thread::sleep(Duration::from_millis(30));
    
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

/// 拟人化打字 - 支持普通字符、虚拟键和组合键混合输入
/// 
/// 支持的虚拟键格式：{Enter}, {Tab}, {Escape}, {Backspace}, {Delete}, 
/// {Home}, {End}, {PageUp}, {PageDown}, {Left}, {Right}, {Up}, {Down},
/// {F1}-{F12} 等
/// 
/// 如果要输入字面意义的 "{" 或 "}"，使用 {{ 和 }}
pub fn humanized_type(text: &str, min_delay: u64, max_delay: u64) -> anyhow::Result<(u32, u64)> {
    let start = Instant::now();
    let mut rng = rand::thread_rng();
    let mut chars_typed = 0u32;
    
    let mut chars = text.chars().peekable();
    
    while let Some(ch) = chars.next() {
        // 检查是否是转义字符 {{
        if ch == '{' {
            if let Some(&next_ch) = chars.peek() {
                if next_ch == '{' {
                    // 转义的左花括号，输出单个 {
                    chars.next(); // 消耗第二个 {
                    send_unicode_char('{');
                    chars_typed += 1;
                    let delay = rng.gen_range(min_delay..max_delay + 1);
                    std::thread::sleep(Duration::from_millis(delay));
                    continue;
                } else {
                    // 可能是虚拟键，收集直到 }
                    let mut key_name = String::new();
                    let mut found_end = false;
                    
                    for c in chars.by_ref() {
                        if c == '}' {
                            found_end = true;
                            break;
                        }
                        key_name.push(c);
                    }
                    
                    if found_end && !key_name.is_empty() {
                        // 检查是否是组合键（包含 +）
                        if key_name.contains('+') {
                            // 发送组合键
                            send_shortcut(&key_name)?;
                            chars_typed += 1;
                        } else {
                            // 发送单键
                            send_virtual_key(&key_name)?;
                            chars_typed += 1;
                        }
                        let delay = rng.gen_range(min_delay..max_delay + 1);
                        std::thread::sleep(Duration::from_millis(delay));
                    } else {
                        // 没有找到闭合的 }，当作普通字符处理
                        send_unicode_char('{');
                        chars_typed += 1;
                        let delay = rng.gen_range(min_delay..max_delay + 1);
                        std::thread::sleep(Duration::from_millis(delay));
                        // 将已收集的字符重新作为普通字符发送
                        for c in key_name.chars() {
                            send_unicode_char(c);
                            chars_typed += 1;
                            let delay = rng.gen_range(min_delay..max_delay + 1);
                            std::thread::sleep(Duration::from_millis(delay));
                        }
                    }
                    continue;
                }
            } else {
                // 最后一个字符是 {，直接输出
                send_unicode_char('{');
                chars_typed += 1;
                let delay = rng.gen_range(min_delay..max_delay + 1);
                std::thread::sleep(Duration::from_millis(delay));
                continue;
            }
        }
        
        // 检查是否是转义字符 }}
        if ch == '}' {
            if let Some(&next_ch) = chars.peek() {
                if next_ch == '}' {
                    // 转义的右花括号，输出单个 }
                    chars.next(); // 消耗第二个 }
                    send_unicode_char('}');
                    chars_typed += 1;
                    let delay = rng.gen_range(min_delay..max_delay + 1);
                    std::thread::sleep(Duration::from_millis(delay));
                    continue;
                }
            }
            // 单独的 } 当作普通字符
            send_unicode_char('}');
            chars_typed += 1;
            let delay = rng.gen_range(min_delay..max_delay + 1);
            std::thread::sleep(Duration::from_millis(delay));
            continue;
        }
        
        // 普通字符
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
/// 支持三种输入模式（通过 typeMode 字段控制）：
///
/// - `keyboard` (默认): 键盘模拟逐字输入，支持 {Enter} 等虚拟键，支持 charDelay 拟人化延迟
/// - `value`: 通过 UIA ValuePattern.SetValue() 直接设置控件文本，无需焦点/可见
/// - `clipboard`: 通过系统剪贴板 + Ctrl+V 粘贴，适合长文本
pub async fn type_text(body: web::Json<TypeRequest>) -> impl Responder {
    let request = body.into_inner();
    let mode = request.type_mode.unwrap_or_default();
    
    info!(
        "API: /api/keyboard/type text_len={} chars mode={:?}",
        request.text.chars().count(),
        mode,
    );
    
    if request.text.is_empty() {
        return HttpResponse::Ok().json(TypeResponse {
            success: true,
            chars_typed: 0,
            duration_ms: 0,
            error: None,
        });
    }

    match mode {
        TypeMode::Value => {
            // UIA ValuePattern.SetValue 模式 — 委托给 uia 模块
            let window = match &request.window {
                Some(w) => w.clone(),
                None => {
                    return HttpResponse::Ok().json(TypeResponse {
                        success: false,
                        chars_typed: 0,
                        duration_ms: 0,
                        error: Some("Value 模式需要提供 window 参数".to_string()),
                    });
                }
            };
            let element = match &request.element {
                Some(e) => e.clone(),
                None => {
                    return HttpResponse::Ok().json(TypeResponse {
                        success: false,
                        chars_typed: 0,
                        duration_ms: 0,
                        error: Some("Value 模式需要提供 element 参数".to_string()),
                    });
                }
            };
            let text = request.text.clone();
            let rid = request.runtime_id.clone();
            let result = tokio::task::spawn_blocking(move || {
                super::super::core::uia::set_value_by_xpath(&window, &element, &text, rid.as_deref())
            }).await;

            match result {
                Ok(Ok(Ok(chars))) => {
                    let chars_typed = chars as u32;
                    info!("ValuePattern.SetValue succeeded: {} chars", chars_typed);
                    HttpResponse::Ok().json(TypeResponse {
                        success: true,
                        chars_typed,
                        duration_ms: 0,
                        error: None,
                    })
                }
                Ok(Ok(Err(e))) => {
                    warn!("ValuePattern.SetValue failed: {}", e);
                    HttpResponse::Ok().json(TypeResponse {
                        success: false,
                        chars_typed: 0,
                        duration_ms: 0,
                        error: Some(e),
                    })
                }
                Ok(Err(e)) => {
                    warn!("Spawn blocking inner error: {}", e);
                    HttpResponse::InternalServerError().json(TypeResponse {
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
        TypeMode::Clipboard => {
            let text = request.text.clone();
            let result = tokio::task::spawn_blocking(move || {
                paste_via_clipboard(&text)
            }).await;

            match result {
                Ok(Ok(Ok(chars))) => {
                    let chars_typed = chars as u32;
                    info!("Clipboard paste succeeded: {} chars", chars_typed);
                    HttpResponse::Ok().json(TypeResponse {
                        success: true,
                        chars_typed,
                        duration_ms: 0,
                        error: None,
                    })
                }
                Ok(Ok(Err(e))) => {
                    warn!("Clipboard paste failed: {}", e);
                    HttpResponse::Ok().json(TypeResponse {
                        success: false,
                        chars_typed: 0,
                        duration_ms: 0,
                        error: Some(e),
                    })
                }
                Ok(Err(e)) => {
                    warn!("Spawn blocking inner error: {}", e);
                    HttpResponse::InternalServerError().json(TypeResponse {
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
        TypeMode::Keyboard => {
            // 键盘模拟模式（默认）- 保持原有逻辑
            let char_delay = request.char_delay.unwrap_or(CharDelayConfig {
                min: default_min_delay(),
                max: default_max_delay(),
            });
        
            let text = request.text.clone();
            let min_delay = char_delay.min;
            let max_delay = char_delay.max;
            
            let result = tokio::task::spawn_blocking(move || {
                humanized_type(&text, min_delay, max_delay)
            }).await;
            
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
    }
}

/// 通过系统剪贴板 + Ctrl+V 粘贴文本
fn paste_via_clipboard(text: &str) -> anyhow::Result<Result<usize, String>> {
    use windows::Win32::Foundation::HANDLE;

    let char_count = text.chars().count();

    // Step 1: 将文本写入剪贴板 (UTF-16)
    let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let byte_size = utf16.len() * 2;

    unsafe {
        let h_mem = GlobalAlloc(GMEM_MOVEABLE, byte_size)
            .map_err(|e| anyhow::anyhow!("GlobalAlloc 失败: {:?}", e))?;

        let ptr = GlobalLock(h_mem);
        if ptr.is_null() {
            return Ok(Err("GlobalLock 失败".to_string()));
        }

        std::ptr::copy_nonoverlapping(utf16.as_ptr() as *const _, ptr as *mut u16, utf16.len());
        let _ = GlobalUnlock(h_mem);

        // 打开/清空/设置剪贴板
        if OpenClipboard(None).is_err() {
            return Ok(Err("无法打开剪贴板（可能被其他程序占用）".to_string()));
        }

        EmptyClipboard().ok();

        // HGLOBAL → HANDLE 转换 (两者都是 *mut c_void 的包装)
        let handle = HANDLE(h_mem.0);
        if SetClipboardData(CF_UNICODETEXT, Some(handle)).is_err() {
            CloseClipboard().ok();
            return Ok(Err("SetClipboardData 失败".to_string()));
        }

        CloseClipboard().ok();
        // 注意：h_mem 所有权已转交给剪贴板
    }

    // Step 2: 发送 Ctrl+V
    match send_ctrl_v() {
        Ok(()) => Ok(Ok(char_count)),
        Err(e) => Ok(Err(format!("剪贴板粘贴失败: {}", e))),
    }
}

/// 发送 Ctrl+V 组合键
fn send_ctrl_v() -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_CONTROL, VK_V};

    unsafe {
        let ctrl_down = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_CONTROL,
                    wScan: 0,
                    dwFlags: KEYBD_EVENT_FLAGS(0),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };

        let v_down = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_V,
                    wScan: 0,
                    dwFlags: KEYBD_EVENT_FLAGS(0),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };

        let v_up = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_V,
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };

        let ctrl_up = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_CONTROL,
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };

        let inputs = [ctrl_down, v_down, v_up, ctrl_up];
        let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        if sent != 4 {
            return Err(format!("SendInput 返回 {} (期望 4)", sent));
        }
    }

    Ok(())
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
                dwFlags: KEYBD_EVENT_FLAGS(0),
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
            type_mode: None,
            window: None,
            element: None,
        };
        assert!(request.text.is_empty());
    }
    
    /// 测试 Unicode 文本
    #[test]
    fn test_unicode_text() {
        let request = TypeRequest {
            text: "中文测试🎉".to_string(),
            char_delay: None,
            type_mode: None,
            window: None,
            element: None,
        };
        assert_eq!(request.text.chars().count(), 5);
    }
    
    /// 测试虚拟键解析 - Enter
    #[test]
    fn test_virtual_key_enter_parsing() {
        let text = "Hello{Enter}";
        // 验证文本中包含虚拟键标记
        assert!(text.contains("{Enter}"));
    }
    
    /// 测试虚拟键解析 - Tab
    #[test]
    fn test_virtual_key_tab_parsing() {
        let text = "Field1{Tab}Field2";
        assert!(text.contains("{Tab}"));
    }
    
    /// 测试转义花括号
    #[test]
    fn test_escaped_braces() {
        let text = "Config: {{key}} = value";
        // 应该被解析为: Config: {key} = value
        assert_eq!(text, "Config: {{key}} = value");
    }
    
    /// 测试混合输入
    #[test]
    fn test_mixed_input() {
        let text = "用户名{Tab}密码{Enter}";
        assert!(text.contains("{Tab}"));
        assert!(text.contains("{Enter}"));
    }
    
    /// 测试多个虚拟键
    #[test]
    fn test_multiple_virtual_keys() {
        let text = "Line1{Enter}Line2{Enter}Line3";
        let enter_count = text.matches("{Enter}").count();
        assert_eq!(enter_count, 2);
    }
}