use crossbeam_channel::{Sender, Receiver, unbounded};
use log::{debug, error, info};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;

use rdev::{grab, Event, EventType, Button};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseEvent {
    LeftClick(i32, i32),
    RightClick(i32, i32),
    MiddleClick(i32, i32),
    RightDoubleClick,
}

pub struct HookState {
    pub active: bool,
    pub report_moves: bool,
    pub click_sender: Sender<MouseEvent>,
    pub moved_sender: Sender<()>,
}

static HOOK_STATE: once_cell::sync::Lazy<Mutex<HookState>> = 
    once_cell::sync::Lazy::new(|| {
        let (click_tx, _) = unbounded();
        let (moved_tx, _) = unbounded();
        Mutex::new(HookState {
            active: false,
            report_moves: false,
            click_sender: click_tx,
            moved_sender: moved_tx,
        })
    });

static CLICK_CHANNEL: once_cell::sync::Lazy<(Sender<MouseEvent>, Mutex<Option<Receiver<MouseEvent>>>)> = 
    once_cell::sync::Lazy::new(|| {
        let (tx, rx) = unbounded();
        (tx, Mutex::new(Some(rx)))
    });

static MOVED_CHANNEL: once_cell::sync::Lazy<(Sender<()>, Mutex<Option<Receiver<()>>>)> = 
    once_cell::sync::Lazy::new(|| {
        let (tx, rx) = unbounded();
        (tx, Mutex::new(Some(rx)))
    });

static MOUSE_STATE: once_cell::sync::Lazy<Mutex<(i32, i32, u64)>> = 
    once_cell::sync::Lazy::new(|| Mutex::new((0, 0, 0)));

static GRAB_RUNNING: AtomicBool = AtomicBool::new(false);

static LAST_RIGHT_CLICK_TIME: AtomicU64 = AtomicU64::new(0);

static SWALLOW_LEFT: AtomicBool = AtomicBool::new(false);
static SWALLOW_RIGHT: AtomicBool = AtomicBool::new(false);
static SWALLOW_MIDDLE: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
fn is_ctrl_pressed() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
    unsafe {
        let state = GetKeyState(17);  // VK_CONTROL = 17
        (state as u16) & 0x8000 != 0
    }
}

#[cfg(not(windows))]
fn is_ctrl_pressed() -> bool {
    false
}

fn detect_right_double_click() -> bool {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    
    let last = LAST_RIGHT_CLICK_TIME.load(Ordering::SeqCst);
    LAST_RIGHT_CLICK_TIME.store(now, Ordering::SeqCst);
    
    now - last < 500 && last > 0
}

pub fn init() -> anyhow::Result<()> {
    let click_sender = CLICK_CHANNEL.0.clone();
    let moved_sender = MOVED_CHANNEL.0.clone();
    
    {
        let mut state = HOOK_STATE.lock();
        state.click_sender = click_sender;
        state.moved_sender = moved_sender;
        state.active = false;
        state.report_moves = false;
    }
    
    if GRAB_RUNNING.load(Ordering::SeqCst) {
        debug!("Grab thread already running, skipping spawn");
        return Ok(());
    }
    
    GRAB_RUNNING.store(true, Ordering::SeqCst);
    debug!("Starting grab thread...");
    
    thread::spawn(|| {
        debug!("Grab thread started");
        
        let callback = |event: Event| -> Option<Event> {
            let state = HOOK_STATE.lock();
            
            match event.event_type {
                EventType::ButtonPress(button) => {
                    if state.active {
                        let (x, y, _) = *MOUSE_STATE.lock();
                        
                        match button {
                            Button::Left => {
                                if is_ctrl_pressed() {
                                    info!("Ctrl+Left click at ({}, {})", x, y);
                                    let _ = state.click_sender.send(MouseEvent::LeftClick(x, y));
                                    SWALLOW_LEFT.store(true, Ordering::SeqCst);
                                    return None;
                                }
                            }
                            Button::Right => {
                                if is_ctrl_pressed() {
                                    if detect_right_double_click() {
                                        info!("Ctrl+Right double click - exit capture mode");
                                        let _ = state.click_sender.send(MouseEvent::RightDoubleClick);
                                        SWALLOW_RIGHT.store(true, Ordering::SeqCst);
                                        return None;
                                    } else {
                                        info!("Ctrl+Right click at ({}, {})", x, y);
                                        let _ = state.click_sender.send(MouseEvent::RightClick(x, y));
                                        SWALLOW_RIGHT.store(true, Ordering::SeqCst);
                                        return None;
                                    }
                                }
                            }
                            Button::Middle => {
                                if is_ctrl_pressed() {
                                    info!("Ctrl+Middle click at ({}, {})", x, y);
                                    let _ = state.click_sender.send(MouseEvent::MiddleClick(x, y));
                                    SWALLOW_MIDDLE.store(true, Ordering::SeqCst);
                                    return None;
                                }
                            }
                            _ => {}
                        }
                    }
                    Some(event)
                }
                
                EventType::ButtonRelease(button) => {
                    match button {
                        Button::Left => {
                            if SWALLOW_LEFT.load(Ordering::SeqCst) {
                                SWALLOW_LEFT.store(false, Ordering::SeqCst);
                                return None;
                            }
                        }
                        Button::Right => {
                            if SWALLOW_RIGHT.load(Ordering::SeqCst) {
                                SWALLOW_RIGHT.store(false, Ordering::SeqCst);
                                return None;
                            }
                        }
                        Button::Middle => {
                            if SWALLOW_MIDDLE.load(Ordering::SeqCst) {
                                SWALLOW_MIDDLE.store(false, Ordering::SeqCst);
                                return None;
                            }
                        }
                        _ => {}
                    }
                    Some(event)
                }
                
                EventType::MouseMove { x, y } => {
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64;

                    // Always update mouse state (needed for get_mouse_state() even outside capture)
                    *MOUSE_STATE.lock() = (x as i32, y as i32, now_ms);

                    if state.report_moves {
                        
                        thread_local! {
                            static LAST_MOVE_SENT: std::cell::Cell<u64> = std::cell::Cell::new(0);
                        }
                        
                        let last_sent = LAST_MOVE_SENT.with(|cell| cell.get());
                        if now_ms - last_sent >= 50 {
                            if state.moved_sender.send(()).is_ok() {
                                LAST_MOVE_SENT.with(|cell| cell.set(now_ms));
                            }
                        }
                    }
                    Some(event)
                }
                
                _ => Some(event)
            }
        };
        
        if let Err(e) = grab(callback) {
            error!("Grab error: {:?}", e);
            GRAB_RUNNING.store(false, Ordering::SeqCst);
        }
        info!("Grab thread ended");
    });
    
    std::thread::sleep(std::time::Duration::from_millis(100));
    
    if GRAB_RUNNING.load(Ordering::SeqCst) {
        info!("Input hook system initialized");
        Ok(())
    } else {
        anyhow::bail!("Failed to start grab thread")
    }
}

pub fn activate_capture() {
    let mut state = HOOK_STATE.lock();
    state.active = true;
    state.report_moves = true;
    LAST_RIGHT_CLICK_TIME.store(0, Ordering::SeqCst);
    info!("Capture activated");
}

pub fn deactivate_capture() {
    let mut state = HOOK_STATE.lock();
    state.active = false;
    state.report_moves = false;
    
    // 重置 swallow 标志，防止取消捕获时鼠标按下状态残留
    SWALLOW_LEFT.store(false, Ordering::SeqCst);
    SWALLOW_RIGHT.store(false, Ordering::SeqCst);
    SWALLOW_MIDDLE.store(false, Ordering::SeqCst);
    
    // 重置双击检测时间
    LAST_RIGHT_CLICK_TIME.store(0, Ordering::SeqCst);
    
    debug!("Capture deactivated");
}

#[allow(dead_code)]
pub fn is_active() -> bool {
    HOOK_STATE.lock().active
}

pub fn poll_mouse_click() -> Option<MouseEvent> {
    CLICK_CHANNEL.1.lock().as_ref().and_then(|rx| rx.try_recv().ok())
}

#[allow(dead_code)]
pub fn poll_mouse_moved() -> Option<()> {
    MOVED_CHANNEL.1.lock().as_ref().and_then(|rx| rx.try_recv().ok())
}

pub fn get_mouse_state() -> (i32, i32, u64) {
    *MOUSE_STATE.lock()
}

#[allow(dead_code)]
pub fn cleanup() {
    GRAB_RUNNING.store(false, Ordering::SeqCst);
    *CLICK_CHANNEL.1.lock() = None;
    *MOVED_CHANNEL.1.lock() = None;
    HOOK_STATE.lock().active = false;
    info!("Input hook system cleaned up");
}