// src/core/narrator.rs
//
// Narrator RunningState registry management.
// Sets HKCU\Software\Microsoft\Narrator\NoRoam\RunningState = 1 to enable
// full UI Automation tree visibility for RPA element picking.

use log::{info, warn};

#[cfg(windows)]
mod win {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::System::Registry::{
        RegCloseKey, RegGetValueW, RegOpenKeyExW, RegSetValueExW,
        HKEY, KEY_READ, KEY_SET_VALUE, RRF_RT_DWORD, REG_DWORD, HKEY_CURRENT_USER,
    };
    use windows::Win32::Foundation::WIN32_ERROR;
    use windows::core::PCWSTR;

    const NARRATOR_KEY_PATH: &str = "Software\\Microsoft\\Narrator\\NoRoam";
    const NARRATOR_VALUE_NAME: &str = "RunningState";

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    fn fmt_err(code: WIN32_ERROR) -> String {
        format!("registry error: {}", code.0)
    }

    fn open_key() -> Result<HKEY, String> {
        let path = to_wide(NARRATOR_KEY_PATH);
        let mut hkey = HKEY::default();
        let code = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR::from_raw(path.as_ptr()),
                Some(0),
                KEY_READ | KEY_SET_VALUE,
                &mut hkey,
            )
        };
        if code == WIN32_ERROR(0) {
            Ok(hkey)
        } else {
            Err(fmt_err(code))
        }
    }

    pub fn read_running_state() -> Result<u32, String> {
        let hkey = open_key()?;
        let name = to_wide(NARRATOR_VALUE_NAME);
        let mut value: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        let code = unsafe {
            RegGetValueW(
                hkey,
                PCWSTR::null(),
                PCWSTR::from_raw(name.as_ptr()),
                RRF_RT_DWORD,
                None,
                Some(&mut value as *mut _ as *mut _),
                Some(&mut size),
            )
        };
        unsafe { let _ = RegCloseKey(hkey); }
        if code == WIN32_ERROR(0) {
            Ok(value)
        } else {
            Err(fmt_err(code))
        }
    }

    pub fn set_running_state(val: u32) -> Result<(), String> {
        let hkey = open_key()?;
        let name = to_wide(NARRATOR_VALUE_NAME);
        let code = unsafe {
            RegSetValueExW(
                hkey,
                PCWSTR::from_raw(name.as_ptr()),
                Some(0),
                REG_DWORD,
                Some(std::slice::from_raw_parts(
                    &val as *const u32 as *const u8,
                    std::mem::size_of::<u32>(),
                )),
            )
        };
        unsafe { let _ = RegCloseKey(hkey); }
        if code == WIN32_ERROR(0) {
            Ok(())
        } else {
            Err(fmt_err(code))
        }
    }
}

/// Guard that restores the previous Narrator RunningState on drop.
pub struct NarratorStateGuard {
    previous_value: Option<u32>,
}

impl Drop for NarratorStateGuard {
    fn drop(&mut self) {
        if let Some(prev) = self.previous_value {
            info!("Restoring Narrator RunningState to {prev}");
            if let Err(e) = set_running_state(prev) {
                warn!("Failed to restore Narrator RunningState: {e}");
            }
        } else {
            info!("No previous Narrator RunningState to restore");
        }
    }
}

/// Enable Narrator RunningState. Reads current value first so it can be
/// restored on drop. Returns a guard that handles restoration.
pub fn enable_narrator_running_state() -> NarratorStateGuard {
    let previous = read_running_state().ok();
    info!("Enabling Narrator RunningState (previous: {previous:?})");
    if let Err(e) = set_running_state(1) {
        warn!("Failed to set Narrator RunningState: {e}");
    }
    NarratorStateGuard { previous_value: previous }
}

#[cfg(windows)]
fn read_running_state() -> Result<u32, String> {
    win::read_running_state()
}

#[cfg(windows)]
fn set_running_state(val: u32) -> Result<(), String> {
    win::set_running_state(val)
}

#[cfg(not(windows))]
fn read_running_state() -> Result<u32, String> {
    Err("Registry not available on non-Windows platform".to_string())
}

#[cfg(not(windows))]
fn set_running_state(_val: u32) -> Result<(), String> {
    Err("Registry not available on non-Windows platform".to_string())
}

/// Check whether narrator motion should be enabled based on environment variables.
/// Returns true if it should run (default).
pub fn should_enable() -> bool {
    if let Ok(val) = std::env::var("NARRATOR_RUNNING_STATE") {
        return val != "0" && val != "false";
    }
    true
}
