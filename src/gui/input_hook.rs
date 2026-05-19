use crossbeam_channel::{Sender, Receiver, unbounded};
use log::{debug, error, info};
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use rdev::{grab, Event, EventType, Key};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEvent {
    CtrlPress,
    ShiftPress,
    AltPress,
    EscapePress,
}

#[derive(Debug, Clone, Copy)]
pub struct MouseMovedEvent;

pub struct HookState {
    pub active: bool,
    pub swallow: bool,
    pub report_moves: bool,
    pub ctrl_pressed: bool,
    pub shift_pressed: bool,
    pub alt_pressed: bool,
    pub key_sender: Sender<KeyEvent>,
    pub moved_sender: Sender<MouseMovedEvent>,
}

static HOOK_STATE: once_cell::sync::Lazy<Arc<Mutex<HookState>>> = 
    once_cell::sync::Lazy::new(|| {
        let (key_tx, _) = unbounded();
        let (moved_tx, _) = unbounded();
        Arc::new(Mutex::new(HookState {
            active: false,
            swallow: true,
            report_moves: false,
            ctrl_pressed: false,
            shift_pressed: false,
            alt_pressed: false,
            key_sender: key_tx,
            moved_sender: moved_tx,
        }))
    });

static KEY_CHANNEL: once_cell::sync::Lazy<(Sender<KeyEvent>, Mutex<Option<Receiver<KeyEvent>>>)> = 
    once_cell::sync::Lazy::new(|| {
        let (tx, rx) = unbounded();
        (tx, Mutex::new(Some(rx)))
    });

static MOVED_CHANNEL: once_cell::sync::Lazy<(Sender<MouseMovedEvent>, Mutex<Option<Receiver<MouseMovedEvent>>>)> = 
    once_cell::sync::Lazy::new(|| {
        let (tx, rx) = unbounded();
        (tx, Mutex::new(Some(rx)))
    });

static MOUSE_STATE: once_cell::sync::Lazy<Mutex<(i32, i32, u64)>> = 
    once_cell::sync::Lazy::new(|| Mutex::new((0, 0, 0)));

static GRAB_RUNNING: AtomicBool = AtomicBool::new(false);

pub fn init() -> anyhow::Result<()> {
    let (key_sender, key_receiver) = unbounded();
    let (moved_sender, moved_receiver) = unbounded();
    
    {
        let mut state = HOOK_STATE.lock();
        state.key_sender = key_sender;
        state.moved_sender = moved_sender;
        state.active = false;
        state.swallow = true;
        state.report_moves = false;
        state.ctrl_pressed = false;
        state.shift_pressed = false;
        state.alt_pressed = false;
    }
    
    *KEY_CHANNEL.1.lock() = Some(key_receiver);
    *MOVED_CHANNEL.1.lock() = Some(moved_receiver);
    
    if GRAB_RUNNING.load(Ordering::SeqCst) {
        debug!("Grab thread already running");
        return Ok(());
    }
    
    GRAB_RUNNING.store(true, Ordering::SeqCst);
    
    thread::spawn(|| {
        debug!("Grab thread starting");
        
        let callback = |event: Event| -> Option<Event> {
            let mut state = HOOK_STATE.lock();
            
            match event.event_type {
                EventType::KeyPress(key) => {
                    match key {
                        Key::ControlLeft | Key::ControlRight => {
                            state.ctrl_pressed = true;
                            if state.active {
                                debug!("Ctrl pressed in capture mode");
                                let _ = state.key_sender.send(KeyEvent::CtrlPress);
                                return None;
                            }
                        }
                        Key::ShiftLeft | Key::ShiftRight => {
                            state.shift_pressed = true;
                            if state.active {
                                debug!("Shift pressed in capture mode");
                                let _ = state.key_sender.send(KeyEvent::ShiftPress);
                                return None;
                            }
                        }
                        Key::Alt | Key::AltGr => {
                            state.alt_pressed = true;
                            if state.active {
                                debug!("Alt pressed in capture mode");
                                let _ = state.key_sender.send(KeyEvent::AltPress);
                                return None;
                            }
                        }
                        Key::Escape => {
                            if state.active {
                                info!("Escape pressed: exiting capture mode");
                                let _ = state.key_sender.send(KeyEvent::EscapePress);
                                state.active = false;
                                state.report_moves = false;
                                return None;
                            }
                        }
                        Key::F4 => {
                            if state.ctrl_pressed && state.shift_pressed {
                                info!("Ctrl+Shift+F4: activating capture mode");
                                state.active = true;
                                state.swallow = true;
                                state.report_moves = true;
                                return None;
                            }
                        }
                        _ => {}
                    }
                    
                    if state.active && state.swallow {
                        if matches!(key, Key::ControlLeft | Key::ControlRight | Key::ShiftLeft | Key::ShiftRight | Key::Alt | Key::AltGr) {
                            return None;
                        }
                    }
                }
                
                EventType::KeyRelease(key) => {
                    match key {
                        Key::ControlLeft | Key::ControlRight => {
                            state.ctrl_pressed = false;
                        }
                        Key::ShiftLeft | Key::ShiftRight => {
                            state.shift_pressed = false;
                        }
                        Key::Alt | Key::AltGr => {
                            state.alt_pressed = false;
                        }
                        _ => {}
                    }
                    
                    if state.active && state.swallow {
                        if matches!(key, Key::ControlLeft | Key::ControlRight | Key::ShiftLeft | Key::ShiftRight | Key::Alt | Key::AltGr) {
                            return None;
                        }
                    }
                }
                
                EventType::MouseMove { x, y } => {
                    if state.report_moves {
                        let now_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_millis() as u64;
                        
                        *MOUSE_STATE.lock() = (x as i32, y as i32, now_ms);
                        
                        thread_local! {
                            static LAST_MOVE_SENT: std::cell::Cell<u64> = std::cell::Cell::new(0);
                        }
                        
                        let last_sent = LAST_MOVE_SENT.with(|cell| cell.get());
                        if now_ms - last_sent >= 50 {
                            if state.moved_sender.send(MouseMovedEvent).is_ok() {
                                LAST_MOVE_SENT.with(|cell| cell.set(now_ms));
                            }
                        }
                    }
                }
                
                _ => {}
            }
            
            Some(event)
        };
        
        if let Err(e) = grab(callback) {
            error!("Grab error: {:?}", e);
            GRAB_RUNNING.store(false, Ordering::SeqCst);
        }
    });
    
    std::thread::sleep(std::time::Duration::from_millis(100));
    
    if GRAB_RUNNING.load(Ordering::SeqCst) {
        info!("Input hook system initialized");
        Ok(())
    } else {
        anyhow::bail!("Failed to start grab thread")
    }
}

pub fn activate_capture(swallow: bool) {
    let mut state = HOOK_STATE.lock();
    state.active = true;
    state.swallow = swallow;
    state.report_moves = true;
    debug!("Capture activated (swallow={})", swallow);
}

pub fn deactivate_capture() {
    let mut state = HOOK_STATE.lock();
    state.active = false;
    state.report_moves = false;
    state.ctrl_pressed = false;
    state.shift_pressed = false;
    state.alt_pressed = false;
    debug!("Capture deactivated");
}

#[allow(dead_code)]
pub fn is_active() -> bool {
    HOOK_STATE.lock().active
}

#[allow(dead_code)]
pub fn cleanup() {
    GRAB_RUNNING.store(false, Ordering::SeqCst);
    *KEY_CHANNEL.1.lock() = None;
    *MOVED_CHANNEL.1.lock() = None;
    HOOK_STATE.lock().active = false;
    info!("Input hook system cleaned up");
}

pub fn poll_key() -> Option<KeyEvent> {
    KEY_CHANNEL.1.lock().as_ref().and_then(|rx| rx.try_recv().ok())
}

#[allow(dead_code)]
pub fn poll_mouse_moved() -> Option<MouseMovedEvent> {
    MOVED_CHANNEL.1.lock().as_ref().and_then(|rx| rx.try_recv().ok())
}

pub fn get_mouse_state() -> (i32, i32, u64) {
    *MOUSE_STATE.lock()
}

#[allow(dead_code)]
pub fn is_alt_pressed() -> bool {
    HOOK_STATE.lock().alt_pressed
}