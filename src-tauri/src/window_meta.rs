use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct WindowMetadata {
    pub window_title: String,
    pub process_name: String,
    pub process_id: u32,
}

impl WindowMetadata {
    pub fn unknown(process_id: u32) -> Self {
        Self {
            window_title: "Unknown".to_string(),
            process_name: "Unknown".to_string(),
            process_id,
        }
    }
}

#[derive(Debug, Error)]
pub enum WindowMetaError {
    #[error("no active window is available")]
    NoActiveWindow,
    #[error("window metadata is not supported on this platform")]
    UnsupportedPlatform,
    #[cfg(target_os = "windows")]
    #[error("windows api call failed")]
    Windows(#[from] windows::core::Error),
}

#[cfg(target_os = "windows")]
pub fn get_active_window_metadata() -> Result<WindowMetadata, WindowMetaError> {
    use windows::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId},
    };

    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0 == 0 {
            return Err(WindowMetaError::NoActiveWindow);
        }

        let mut title_buf = vec![0_u16; 512];
        let title_len = GetWindowTextW(hwnd, &mut title_buf);
        let window_title = if title_len > 0 {
            String::from_utf16_lossy(&title_buf[..title_len as usize])
        } else {
            "Unknown".to_string()
        };

        let mut process_id = 0_u32;
        GetWindowThreadProcessId(hwnd, Some(&mut process_id as *mut u32));

        let process_name = get_process_name(process_id).unwrap_or_else(|_| "Unknown".to_string());

        Ok(WindowMetadata {
            window_title,
            process_name,
            process_id,
        })
    }
}

#[cfg(target_os = "windows")]
fn get_process_name(process_id: u32) -> Result<String, WindowMetaError> {
    use windows::Win32::{
        Foundation::{CloseHandle, HMODULE},
        System::{
            ProcessStatus::GetModuleBaseNameW,
            Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ},
        },
    };

    unsafe {
        let handle = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
            false,
            process_id,
        )?;

        let mut process_name_buf = vec![0_u16; 512];
        let process_name_len =
            GetModuleBaseNameW(handle, HMODULE::default(), &mut process_name_buf);
        let process_name = if process_name_len > 0 {
            String::from_utf16_lossy(&process_name_buf[..process_name_len as usize])
        } else {
            "Unknown".to_string()
        };

        let _ = CloseHandle(handle);

        Ok(process_name)
    }
}

#[cfg(not(target_os = "windows"))]
pub fn get_active_window_metadata() -> Result<WindowMetadata, WindowMetaError> {
    Err(WindowMetaError::UnsupportedPlatform)
}

#[cfg(test)]
mod tests {
    use super::WindowMetadata;

    #[test]
    fn unknown_metadata_uses_fallback_labels() {
        let metadata = WindowMetadata::unknown(42);

        assert_eq!(metadata.window_title, "Unknown");
        assert_eq!(metadata.process_name, "Unknown");
        assert_eq!(metadata.process_id, 42);
    }
}
