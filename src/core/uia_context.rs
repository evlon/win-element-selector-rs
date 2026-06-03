// src/core/uia_context.rs
//
// Global UIA context — MTA-based UIAutomation singleton.
//
// Uses uiautomation-rs library for safe, idiomatic COM access.
// Under MTA (COINIT_MULTITHREADED), UIAutomation and UIElement
// are free-threaded and can be safely accessed from any MTA thread.
//
// The global UIAutomation instance is stored via OnceLock, providing
// zero-overhead access after initialization.

use std::sync::OnceLock;
use uiautomation::core::UIAutomation;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

// ═══════════════════════════════════════════════════════════════════════════════
// Global UIAutomation singleton
// ═══════════════════════════════════════════════════════════════════════════════
// Under MTA, UIAutomation is free-threaded: safe to call from any thread.
// OnceLock gives us immutable access without Mutex overhead.
// UIAutomation implements Send + Sync (added in previous step).

static UIA_AUTOMATION: OnceLock<UIAutomation> = OnceLock::new();

// ═══════════════════════════════════════════════════════════════════════════════
// Public API
// ═══════════════════════════════════════════════════════════════════════════════

/// Initialize the global UIA context (MTA mode).
/// Must be called once at program startup, before any UIA operations.
/// Safe to call multiple times — subsequent calls are no-ops.
///
/// IMPORTANT: This spawns a background thread to initialize COM in MTA mode,
/// because the main thread must remain in STA for winit/iced (OleInitialize).
/// Calling CoInitializeEx(MTA) on the main thread would conflict with winit's
/// OleInitialize and cause a panic (RPC_E_CHANGED_MODE).
pub fn init_uia_context() -> anyhow::Result<()> {
    // If already initialized, return immediately
    if UIA_AUTOMATION.get().is_some() {
        log::info!("UiaContext already initialized, skipping");
        return Ok(());
    }

    // Initialize COM and UIAutomation on a dedicated background thread
    // to avoid conflicting with winit's OleInitialize on the main thread.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("uia-mta-init".to_string())
        .spawn(move || {
            // Initialize COM in MTA mode on this background thread
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            if hr.is_err() && hr != windows::core::HRESULT(1) {
                let _ = tx.send(Err(anyhow::anyhow!(
                    "CoInitializeEx(MTA) failed on background thread: HRESULT={:#010x}",
                    hr.0 as u32
                )));
                return;
            }

            // Create the global UIAutomation instance using uiautomation-rs
            // new_direct() does not call CoInitializeEx again (we already did it)
            match UIAutomation::new_direct() {
                Ok(automation) => {
                    let _ = tx.send(Ok(automation));
                }
                Err(e) => {
                    let _ = tx.send(Err(anyhow::anyhow!(
                        "UIAutomation::new_direct() failed: {e}"
                    )));
                }
            }

            // Keep this thread alive for the lifetime of the process
            // so the MTA apartment remains active. UIAutomation is free-threaded
            // and can be used from any MTA thread after this.
            // The thread will be cleaned up when the process exits.
            std::thread::park();
        })
        .map_err(|e| anyhow::anyhow!("Failed to spawn MTA init thread: {e}"))?;

    let automation = rx.recv().map_err(|e| anyhow::anyhow!("MTA init thread panicked: {e}"))??;
    let _ = UIA_AUTOMATION.set(automation);
    log::info!("UiaContext initialized (MTA mode, uiautomation-rs, background thread)");
    Ok(())
}

/// Get a reference to the global UIAutomation instance.
/// Panics if `init_uia_context()` has not been called.
///
/// # Deprecation
/// Prefer `crate::core::uia::helpers::AutomationProvider::get_healthy()` for thread-local
/// IUIAutomation with health checks, which is more robust than the global OnceLock singleton.
#[deprecated(since = "1.2.0", note = "Use crate::core::uia::helpers::AutomationProvider::get_healthy() for thread-local IUIAutomation with health checks. Scheduled removal in v2.0.0")]
pub fn get_automation() -> &'static UIAutomation {
    UIA_AUTOMATION.get().expect("UiaContext not initialized — call init_uia_context() first")
}

/// Try to get a reference to the global UIAutomation instance.
/// Returns `Err` if `init_uia_context()` has not been called, instead of panicking.
pub fn try_get_automation() -> anyhow::Result<&'static UIAutomation> {
    UIA_AUTOMATION
        .get()
        .ok_or_else(|| anyhow::anyhow!("UiaContext not initialized — call init_uia_context() first"))
}

/// Execute a closure with a reference to the global UIAutomation.
/// This is the primary way to access UIA functionality.
#[deprecated(since = "1.2.0", note = "Use crate::core::uia::helpers::AutomationProvider::get_healthy() instead. Scheduled removal in v2.0.0")]
pub fn with_automation<F, R>(f: F) -> R
where
    F: FnOnce(&UIAutomation) -> R,
{
    #[allow(deprecated)]
    let auto = get_automation();
    f(auto)
}

/// Ensure COM is initialized on the current thread in MTA mode.
/// Call this before using UIA from a new thread (e.g., tokio worker threads).
/// This is idempotent — safe to call multiple times.
pub fn ensure_mta() -> anyhow::Result<()> {
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    // S_OK (0) = first init on this thread
    // S_FALSE (1) = already initialized (benign)
    if hr.is_err() && hr != windows::core::HRESULT(1) {
        // RPC_E_CHANGED_MODE (0x80010106) means the thread was initialized in STA
        if hr == windows::core::HRESULT(0x80010106u32 as i32) {
            log::warn!("Thread already initialized in STA mode; MTA init skipped (RPC_E_CHANGED_MODE)");
        } else {
            return Err(anyhow::anyhow!(
                "CoInitializeEx(MTA) failed: HRESULT={:#010x}",
                hr.0 as u32
            ));
        }
    }
    Ok(())
}
