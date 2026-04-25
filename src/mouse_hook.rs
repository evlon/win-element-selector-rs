// src/mouse_hook.rs
//
// Global mouse hook using Windows low-level hook (WH_MOUSE_LL).
// This allows capturing clicks outside the application window and swallowing events.

use crossbeam_channel::{Sender, Receiver, unbounded};
use log::{debug, error, info};
use parking_lot::Mutex;
use std::sync::Arc;

#[cfg(target_os = "windows")]
use windows::Win32::{
    Foundation::{LPARAM, WPARAM, LRESULT},
    UI::WindowsAndMessaging::{
        SetWindowsHookExW, UnhookWindowsHookEx, CallNextHookEx,
        WH_MOUSE_LL, WM_LBUTTONDOWN, WM_LBUTTONUP,
        MSLLHOOKSTRUCT, HHOOK,
        GetMessageW,
    },
    System::LibraryLoader::GetModuleHandleW,
};

// ═══════════════════════════════════════════════════════════════════════════════
// Click event
// ═══════════════════════════════════════════════════════════════════════════════

/// Represents a captured mouse click event.
#[derive(Debug, Clone, Copy)]
pub struct ClickEvent {
    pub x: i32,
    pub y: i32,
    pub is_down: bool,  // true for WM_LBUTTONDOWN, false for WM_LBUTTONUP
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
    /// Channel to send click events to main thread.
    sender: Sender<ClickEvent>,
}

impl HookState {
    fn new(sender: Sender<ClickEvent>) -> Self {
        Self {
            active: false,
            swallow: true,  // Default: swallow clicks to prevent triggering target
            sender,
        }
    }
}

// Global hook state wrapped in Arc<Mutex> for thread-safe access.
static HOOK_STATE: once_cell::sync::Lazy<Arc<Mutex<HookState>>> = 
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(HookState::new(unbounded().0))));

// Global hook handle (only accessed from hook thread).
#[cfg(target_os = "windows")]
thread_local! {
    static HOOK_HANDLE: std::cell::RefCell<Option<HHOOK>> = std::cell::RefCell::new(None);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Windows implementation
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(target_os = "windows")]
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
            return CallNextHookEx(hook, n_code, w_param, l_param);
        }

        // Check if we should capture this event.
        let state = HOOK_STATE.lock();
        if state.active {
            let msg = w_param.0 as u32;
            
            // Only process left button events.
            if msg == WM_LBUTTONDOWN || msg == WM_LBUTTONUP {
                // Extract mouse position from MSLLHOOKSTRUCT.
                let hook_struct = &*(l_param.0 as *const MSLLHOOKSTRUCT);
                let event = ClickEvent {
                    x: hook_struct.pt.x,
                    y: hook_struct.pt.y,
                    is_down: msg == WM_LBUTTONDOWN,
                };
                
                debug!("Mouse hook captured: {:?} at ({}, {})", 
                       if event.is_down { "DOWN" } else { "UP" }, event.x, event.y);
                
                // Send event to main thread (non-blocking).
                if state.sender.send(event).is_ok() {
                    info!("Click event sent to main thread: {:?}", event);
                }
                
                // If swallow mode is enabled, block the event from reaching target.
                if state.swallow && event.is_down {
                    debug!("Swallowing click event (blocking propagation)");
                    // Return non-zero to block the event.
                    return LRESULT(1);
                }
            }
        }

        // Pass to next hook in chain.
        CallNextHookEx(hook, n_code, w_param, l_param)
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
                    module,
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
// Non-Windows stub
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(not(target_os = "windows"))]
pub mod win_hook {
    use super::*;
    pub fn start_hook_thread() -> anyhow::Result<()> {
        info!("Mouse hook not available on non-Windows platforms");
        Ok(())
    }
    pub fn stop_hook_thread() {}
}

// ═══════════════════════════════════════════════════════════════════════════════
// Public API
// ═══════════════════════════════════════════════════════════════════════════════

/// Initialize the mouse hook system.
/// This starts the hook thread that will run the message loop.
pub fn init() -> anyhow::Result<()> {
    // Create a new channel for events.
    let (sender, receiver) = unbounded();
    
    // Update the global state with the new receiver.
    {
        let mut state = HOOK_STATE.lock();
        state.sender = sender;
        state.active = false;
        state.swallow = true;
    }
    
    // Store receiver for later retrieval.
    *RECEIVER.lock() = Some(receiver);
    
    // Start the hook thread.
    win_hook::start_hook_thread()?;
    
    info!("Mouse hook system initialized");
    Ok(())
}

/// Cleanup the mouse hook system.
pub fn cleanup() {
    win_hook::stop_hook_thread();
    *RECEIVER.lock() = None;
    HOOK_STATE.lock().active = false;
    info!("Mouse hook system cleaned up");
}

/// Activate capture mode.
/// When active, clicks will be captured and optionally swallowed.
pub fn activate_capture(swallow: bool) {
    let mut state = HOOK_STATE.lock();
    state.active = true;
    state.swallow = swallow;
    debug!("Capture activated (swallow={})", swallow);
}

/// Deactivate capture mode.
/// Clicks will no longer be captured.
pub fn deactivate_capture() {
    HOOK_STATE.lock().active = false;
    debug!("Capture deactivated");
}

/// Check if capture mode is active.
pub fn is_active() -> bool {
    HOOK_STATE.lock().active
}

/// Get the receiver for click events.
/// The main thread should poll this receiver during the capture state.
pub fn get_receiver() -> Option<Receiver<ClickEvent>> {
    RECEIVER.lock().clone()
}

// Store the receiver separately for easy access.
static RECEIVER: Mutex<Option<Receiver<ClickEvent>>> = Mutex::new(None);

/// Poll for a click event (non-blocking).
/// Returns Some(event) if an event was received, None otherwise.
pub fn poll_click() -> Option<ClickEvent> {
    RECEIVER.lock().as_ref().and_then(|rx| rx.try_recv().ok())
}

/// Wait for a click event with timeout.
/// Returns Some(event) if an event was received within the timeout, None otherwise.
pub fn wait_click_timeout(timeout_ms: u64) -> Option<ClickEvent> {
    RECEIVER.lock().as_ref().and_then(|rx| {
        rx.recv_timeout(std::time::Duration::from_millis(timeout_ms)).ok()
    })
}