// src/gui/raw_input.rs
//
// 全局 ESC 键检测：通过独立后台线程轮询 GetAsyncKeyState。
// GetAsyncKeyState 读取全局键盘状态，不受焦点影响。
// 使用去重逻辑避免重复触发。

use crossbeam_channel::{Receiver, unbounded};
use log::{debug, error, info};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

static VK_ESCAPE: u16 = 0x1B;

static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// 初始化全局 ESC 检测线程，返回接收端。
/// 当检测到 ESC 键按下时，接收端会收到 () 信号。
pub fn init() -> Receiver<()> {
    if INITIALIZED.load(Ordering::SeqCst) {
        let (tx, rx) = unbounded::<()>();
        drop(tx);
        return rx;
    }

    let (esc_tx, esc_rx) = unbounded::<()>();

    let thread_handle = thread::Builder::new()
        .name("raw_input_esc".to_string())
        .spawn(move || {
            info!("[raw_input] ESC 检测线程已启动");
            let mut was_pressed = false;

            loop {
                // 短暂休眠避免 CPU 空转
                thread::sleep(Duration::from_millis(50));

                // 检查线程是否应该退出（通过 INITIALIZED 标志）
                if !INITIALIZED.load(Ordering::SeqCst) {
                    info!("[raw_input] ESC 检测线程退出");
                    break;
                }

                unsafe {
                    let state = GetAsyncKeyState(VK_ESCAPE as i32);
                    let is_pressed = (state as u16) & 0x8000 != 0;

                    // 只在按下瞬间触发一次（边沿检测）
                    if is_pressed && !was_pressed {
                        debug!("[raw_input] ESC 键按下");
                        if esc_tx.send(()).is_err() {
                            info!("[raw_input] 接收端已断开，ESC 线程退出");
                            break;
                        }
                    }

                    was_pressed = is_pressed;
                }
            }
        });

    match thread_handle {
        Ok(_) => {}
        Err(e) => {
            error!("[raw_input] 创建 ESC 检测线程失败: {}", e);
            INITIALIZED.store(false, Ordering::SeqCst);
        }
    }

    INITIALIZED.store(true, Ordering::SeqCst);
    info!("[raw_input] ESC 全局检测已初始化");
    esc_rx
}

pub fn cleanup() {
    if !INITIALIZED.load(Ordering::SeqCst) {
        return;
    }

    INITIALIZED.store(false, Ordering::SeqCst);
    info!("[raw_input] Raw Input 清理完成");
}
