// src/mouse_hook.rs
//
// Global mouse hook using Windows low-level hook (WH_MOUSE_LL).
// This allows capturing clicks outside the application window and swallowing events.

use crossbeam_channel::{Sender, Receiver, unbounded};
use log::{debug, error, info};
use parking_lot::Mutex;
use std::sync::Arc;

use windows::Win32::{
    Foundation::{LPARAM, WPARAM, LRESULT},
    UI::WindowsAndMessaging::{
        SetWindowsHookExW, UnhookWindowsHookEx, CallNextHookEx,
        WH_MOUSE_LL, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
        MSLLHOOKSTRUCT, HHOOK,
        GetMessageW,
    },
    UI::Input::KeyboardAndMouse::{
        GetKeyState, VK_CONTROL, VK_SHIFT,
    },
    System::LibraryLoader::GetModuleHandleW,
};

// ═══════════════════════════════════════════════════════════════════════════════
// Mouse move event for real-time highlight
// ═══════════════════════════════════════════════════════════════════════════════

/// Represents a mouse move event (unused, kept for API compatibility).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub struct MouseMoveEvent {
    pub x: i32,
    pub y: i32,
}

/// Event sent to main thread when mouse stops moving.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub struct MouseStillEvent {
    pub x: i32,
    pub y: i32,
}

/// Signal to clear the current highlight (unused, kept for API compatibility).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub struct MouseMovedEvent;

// ═══════════════════════════════════════════════════════════════════════════════
// Click event
// ═══════════════════════════════════════════════════════════════════════════════

/// Capture mode type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMode {
    /// Normal single element capture (Ctrl + click or simple click)
    Single,
    /// Batch capture similar elements (Shift + click)
    Batch,
    /// No capture (modifier not pressed)
    None,
}

/// Represents a captured mouse click event.
#[derive(Debug, Clone, Copy)]
pub struct ClickEvent {
    pub x: i32,
    pub y: i32,
    pub is_down: bool,  // true for WM_LBUTTONDOWN, false for WM_LBUTTONUP
    pub ctrl_pressed: bool,
    pub shift_pressed: bool,
}

impl ClickEvent {
    /// Determine the capture mode based on keyboard modifiers.
    pub fn capture_mode(&self) -> CaptureMode {
        if self.ctrl_pressed && self.is_down {
            CaptureMode::Single
        } else if self.shift_pressed && self.is_down {
            CaptureMode::Batch
        } else {
            // No modifier pressed - do not capture
            CaptureMode::None
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Hook state (shared between hook thread and main thread)
// ═══════════════════════════════════════════════════════════════════════════════

/// State for the global mouse hook.
pub struct HookState {
    /// Whether the hook is active and should capture clicks.
    active: bool,
    /// Whether to swallow (block) the click event from reaching target.
    swallow: bool,
    /// Whether to report mouse still/moved events (for real-time highlight).
    report_moves: bool,
    /// Channel to send click events to main thread.
    click_sender: Sender<ClickEvent>,
    /// Channel to send mouse still events to main thread.
    still_sender: Sender<MouseStillEvent>,
    /// Channel to send mouse moved events to main thread.
    moved_sender: Sender<MouseMovedEvent>,
}

impl HookState {
    fn new(
        click_sender: Sender<ClickEvent>,
        still_sender: Sender<MouseStillEvent>,
        moved_sender: Sender<MouseMovedEvent>,
    ) -> Self {
        Self {
            active: false,
            swallow: true,  // Default: swallow clicks to prevent triggering target
            report_moves: false,
            click_sender,
            still_sender,
            moved_sender,
        }
    }
}

// Global hook state wrapped in Arc<Mutex> for thread-safe access.
static HOOK_STATE: once_cell::sync::Lazy<Arc<Mutex<HookState>>> = 
    once_cell::sync::Lazy::new(|| {
        let (click_tx, _) = unbounded();
        let (still_tx, _) = unbounded();
        let (moved_tx, _) = unbounded();
        Arc::new(Mutex::new(HookState::new(click_tx, still_tx, moved_tx)))
    });

// Global channels for receiving events
static CLICK_CHANNEL: once_cell::sync::Lazy<(Sender<ClickEvent>, Mutex<Option<Receiver<ClickEvent>>>)> = 
    once_cell::sync::Lazy::new(|| {
        let (tx, rx) = unbounded();
        (tx, Mutex::new(Some(rx)))
    });

static STILL_CHANNEL: once_cell::sync::Lazy<(Sender<MouseStillEvent>, Mutex<Option<Receiver<MouseStillEvent>>>)> = 
    once_cell::sync::Lazy::new(|| {
        let (tx, rx) = unbounded();
        (tx, Mutex::new(Some(rx)))
    });

static MOVED_CHANNEL: once_cell::sync::Lazy<(Sender<MouseMovedEvent>, Mutex<Option<Receiver<MouseMovedEvent>>>)> = 
    once_cell::sync::Lazy::new(|| {
        let (tx, rx) = unbounded();
        (tx, Mutex::new(Some(rx)))
    });

// Global hook handle (only accessed from hook thread).
thread_local! {
    static HOOK_HANDLE: std::cell::RefCell<Option<HHOOK>> = std::cell::RefCell::new(None);
}

// Thread handle for the hook thread
#[allow(dead_code)]
static mut HOOK_THREAD_HANDLE: Option<std::thread::JoinHandle<()>> = None;

// ═══════════════════════════════════════════════════════════════════════════════
// Windows implementation
// ═══════════════════════════════════════════════════════════════════════════════

pub mod win_hook {
    use super::*;
    use std::thread::{self, JoinHandle};
    use std::sync::atomic::{AtomicBool, Ordering};

    static HOOK_THREAD_RUNNING: AtomicBool = AtomicBool::new(false);
    static mut HOOK_THREAD_HANDLE: Option<JoinHandle<()>> = None;

    /// Low-level mouse hook callback.
    /// This function is called by Windows for every mouse event in the system.
    unsafe extern "system" fn mouse_hook_proc(
        n_code: i32,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> LRESULT {
        // Get the hook handle from thread-local storage.
        let hook = HOOK_HANDLE.with(|h| *h.borrow());
        let hook = hook.unwrap_or(HHOOK::default());

        // If nCode < 0, must pass to CallNextHookEx without processing.
        if n_code < 0 {
            return CallNextHookEx(Some(hook), n_code, w_param, l_param);
        }

        // Check if we should capture this event.
        let state = HOOK_STATE.lock();
        if state.active {
            let msg = w_param.0 as u32;
            
            // Track mouse movement with debounce/throttle mechanism.
            // Only update shared state and send events at a controlled rate to avoid flooding.
            if state.report_moves && msg == WM_MOUSEMOVE {
                let hook_struct = &*(l_param.0 as *const MSLLHOOKSTRUCT);
                let x = hook_struct.pt.x;
                let y = hook_struct.pt.y;
                
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;
                
                // Update shared state for main thread to access (always update).
                update_mouse_state(x, y, now_ms);
                
                // Throttle: only send move events every 50ms to avoid flooding the channel
                // This prevents performance issues when there are many rapid mouse movements
                const MOVE_THROTTLE_MS: u64 = 50;
                
                // Use a thread-local variable to track last sent time
                thread_local! {
                    static LAST_MOVE_SENT: std::cell::Cell<u64> = std::cell::Cell::new(0);
                }
                
                let last_sent = LAST_MOVE_SENT.with(|cell| cell.get());
                if now_ms - last_sent >= MOVE_THROTTLE_MS {
                    // Send a lightweight notification that mouse moved
                    // Main thread can then poll get_mouse_state() for actual position
                    if state.moved_sender.send(MouseMovedEvent).is_ok() {
                        LAST_MOVE_SENT.with(|cell| cell.set(now_ms));
                    }
                }
            }
            
            // Only process left button events.
            if msg == WM_LBUTTONDOWN || msg == WM_LBUTTONUP {
                // Extract mouse position from MSLLHOOKSTRUCT.
                let hook_struct = &*(l_param.0 as *const MSLLHOOKSTRUCT);
                
                // Check keyboard modifier states.
                // GetKeyState returns i16, high bit (0x8000) indicates key is pressed.
                let ctrl_state = unsafe { GetKeyState(VK_CONTROL.0.into()) };
                let shift_state = unsafe { GetKeyState(VK_SHIFT.0.into()) };
                let ctrl_pressed = ctrl_state < 0;
                let shift_pressed = shift_state < 0;
                
                let event = ClickEvent {
                    x: hook_struct.pt.x,
                    y: hook_struct.pt.y,
                    is_down: msg == WM_LBUTTONDOWN,
                    ctrl_pressed,
                    shift_pressed,
                };
                
                debug!("Mouse hook captured: {:?} at ({}, {})", 
                       if event.is_down { "DOWN" } else { "UP" }, event.x, event.y);
                
                // Send click event to main thread (non-blocking).
                if state.click_sender.send(event).is_ok() {
                    debug!("Click event sent to main thread");
                }
                
                // Only swallow the event if modifier key is pressed AND swallow mode is enabled.
                // This allows normal clicking when no modifier is pressed.
                if state.swallow && event.is_down && (ctrl_pressed || shift_pressed) {
                    debug!("Swallowing click event with modifier (blocking propagation)");
                    // Return non-zero to block the event.
                    return LRESULT(1);
                }
            }
        }

        // Pass to next hook in chain.
        CallNextHookEx(Some(hook), n_code, w_param, l_param)
    }

    /// Start the hook thread. The hook must be installed in a thread with a message loop.
    pub fn start_hook_thread() -> anyhow::Result<()> {
        if HOOK_THREAD_RUNNING.load(Ordering::SeqCst) {
            debug!("Hook thread already running");
            return Ok(());
        }

        HOOK_THREAD_RUNNING.store(true, Ordering::SeqCst);
        
        let thread = thread::spawn(|| {
            debug!("Hook thread starting");
            
            // Get module handle for current process.
            let module = unsafe { GetModuleHandleW(None) }
                .expect("GetModuleHandleW failed");
            
            // Install low-level mouse hook.
            let hook = unsafe {
                SetWindowsHookExW(
                    WH_MOUSE_LL,
                    Some(mouse_hook_proc),
                    Some(module.into()),
                    0,  // 0 = hook applies to all threads in current desktop
                )
            };
            
            match hook {
                Ok(h) => {
                    info!("Low-level mouse hook installed successfully");
                    HOOK_HANDLE.with(|cell| *cell.borrow_mut() = Some(h));
                    
                    // Message loop - required for low-level hooks to work.
                    // The hook callback is called in the context of this thread.
                    let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
                    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
                        // Just keep the message loop running.
                        // We don't need to process messages, just keep the thread alive.
                    }
                    
                    debug!("Hook thread message loop exiting");
                    
                    // Unhook when thread exits.
                    HOOK_HANDLE.with(|cell| {
                        if let Some(h) = *cell.borrow() {
                            unsafe {
                                if UnhookWindowsHookEx(h).is_ok() {
                                    info!("Mouse hook uninstalled");
                                }
                            }
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to install mouse hook: {:?}", e);
                    HOOK_THREAD_RUNNING.store(false, Ordering::SeqCst);
                }
            }
        });
        
        // Store thread handle for later cleanup.
        // Note: We don't join this thread normally; it runs until WM_QUIT is sent.
        unsafe { HOOK_THREAD_HANDLE = Some(thread); }
        
        // Give the thread a moment to install the hook.
        std::thread::sleep(std::time::Duration::from_millis(50));
        
        if HOOK_THREAD_RUNNING.load(Ordering::SeqCst) {
            Ok(())
        } else {
            anyhow::bail!("Failed to start hook thread")
        }
    }

    /// Stop the hook thread by sending WM_QUIT.
    #[allow(dead_code)]
    pub fn stop_hook_thread() {
        if !HOOK_THREAD_RUNNING.load(Ordering::SeqCst) {
            return;
        }
        
        debug!("Stopping hook thread");
        
        // Send WM_QUIT to the hook thread's message loop.
        // Note: We can't easily get the thread's HWND, so we use a different approach.
        // We'll just mark it as not running and let it exit naturally on app close.
        HOOK_THREAD_RUNNING.store(false, Ordering::SeqCst);
        
        // Try to unhook directly.
        HOOK_HANDLE.with(|cell| {
            if let Some(h) = *cell.borrow() {
                unsafe {
                    if UnhookWindowsHookEx(h).is_ok() {
                        info!("Mouse hook uninstalled on stop");
                    }
                }
            }
        });
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Public API
// ═══════════════════════════════════════════════════════════════════════════════

/// Initialize the mouse hook system.
/// This starts the hook thread that will run the message loop.
pub fn init() -> anyhow::Result<()> {
    // Create new channels for events.
    let (click_sender, click_receiver) = unbounded();
    let (still_sender, still_receiver) = unbounded();
    let (moved_sender, moved_receiver) = unbounded();
    
    // Update the global state with the new receivers.
    {
        let mut state = HOOK_STATE.lock();
        state.click_sender = click_sender;
        state.still_sender = still_sender;
        state.moved_sender = moved_sender;
        state.active = false;
        state.swallow = true;
        state.report_moves = false;
    }
    
    // Store receivers for later retrieval.
    *CLICK_CHANNEL.1.lock() = Some(click_receiver);
    *STILL_CHANNEL.1.lock() = Some(still_receiver);
    *MOVED_CHANNEL.1.lock() = Some(moved_receiver);
    
    // Start the hook thread.
    win_hook::start_hook_thread()?;
    
    info!("Mouse hook system initialized");
    Ok(())
}

/// Cleanup the mouse hook system.
#[allow(dead_code)]
pub fn cleanup() {
    win_hook::stop_hook_thread();
    *CLICK_CHANNEL.1.lock() = None;
    *STILL_CHANNEL.1.lock() = None;
    *MOVED_CHANNEL.1.lock() = None;
    HOOK_STATE.lock().active = false;
    info!("Mouse hook system cleaned up");
}

/// Activate capture mode.
/// When active, clicks will be captured and optionally swallowed.
pub fn activate_capture(swallow: bool) {
    let mut state = HOOK_STATE.lock();
    state.active = true;
    state.swallow = swallow;
    state.report_moves = true;  // Enable mouse move reporting for real-time highlight
    debug!("Capture activated (swallow={})", swallow);
}

/// Deactivate capture mode.
/// Clicks will no longer be captured.
pub fn deactivate_capture() {
    let mut state = HOOK_STATE.lock();
    state.active = false;
    state.report_moves = false;  // Disable mouse move reporting
    debug!("Capture deactivated");
}

/// Check if capture mode is active.
#[allow(dead_code)]
pub fn is_active() -> bool {
    HOOK_STATE.lock().active
}

/// Get the receiver for click events.
/// The main thread should poll this receiver during the capture state.
#[allow(dead_code)]
pub fn get_receiver() -> Option<Receiver<ClickEvent>> {
    CLICK_CHANNEL.1.lock().clone()
}

// Store the receivers separately for easy access (unused).
#[allow(dead_code)]
static CLICK_RECEIVER: Mutex<Option<Receiver<ClickEvent>>> = Mutex::new(None);
#[allow(dead_code)]
static MOVE_RECEIVER: Mutex<Option<Receiver<MouseMoveEvent>>> = Mutex::new(None);

/// Poll for a click event (non-blocking).
/// Returns Some(event) if an event was received, None otherwise.
pub fn poll_click() -> Option<ClickEvent> {
    CLICK_CHANNEL.1.lock().as_ref().and_then(|rx| rx.try_recv().ok())
}

/// Poll for a mouse still event (non-blocking).
/// Returns Some(event) if the mouse has stopped at a position.
#[allow(dead_code)]
pub fn poll_mouse_still() -> Option<MouseStillEvent> {
    STILL_CHANNEL.1.lock().as_ref().and_then(|rx| rx.try_recv().ok())
}

/// Poll for a mouse moved event (non-blocking).
/// Returns Some(event) if the mouse has moved (to clear highlight).
#[allow(dead_code)]
pub fn poll_mouse_moved() -> Option<MouseMovedEvent> {
    MOVED_CHANNEL.1.lock().as_ref().and_then(|rx| rx.try_recv().ok())
}

/// Get the latest mouse position and time from hook thread's thread-local storage.
/// This function must be called from the hook thread, so we expose it differently.
/// Instead, we'll use a shared Mutex for cross-thread communication.
static MOUSE_STATE: once_cell::sync::Lazy<parking_lot::Mutex<(i32, i32, u64)>> = 
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new((0, 0, 0)));

/// Get the latest mouse state (x, y, timestamp_ms).
pub fn get_mouse_state() -> (i32, i32, u64) {
    let state = MOUSE_STATE.lock();
    *state
}

/// Update the mouse state from hook thread.
pub fn update_mouse_state(x: i32, y: i32, time_ms: u64) {
    let mut state = MOUSE_STATE.lock();
    *state = (x, y, time_ms);
}

/// Wait for a click event with timeout.
/// Returns Some(event) if an event was received within the timeout, None otherwise.
#[allow(dead_code)]
pub fn wait_click_timeout(timeout_ms: u64) -> Option<ClickEvent> {
    CLICK_CHANNEL.1.lock().as_ref().and_then(|rx| {
        rx.recv_timeout(std::time::Duration::from_millis(timeout_ms)).ok()
    })
}