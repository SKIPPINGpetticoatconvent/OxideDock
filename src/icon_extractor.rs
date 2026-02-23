use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::UI::Shell::{SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGetFileInfoW};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, HICON};
use windows::core::{HSTRING, PCWSTR};

#[derive(Debug)]
pub struct SystemIcon {
    pub hicon: HICON,
}

impl Drop for SystemIcon {
    fn drop(&mut self) {
        if !self.hicon.is_invalid() {
            unsafe {
                let _ = DestroyIcon(self.hicon);
            }
        }
    }
}

pub fn extract_icon(path: &str) -> Option<SystemIcon> {
    unsafe {
        let mut shfi = SHFILEINFOW::default();
        let path_hstring = HSTRING::from(path);
        let result = SHGetFileInfoW(
            PCWSTR(path_hstring.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut shfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );

        if result != 0 && !shfi.hIcon.is_invalid() {
            Some(SystemIcon { hicon: shfi.hIcon })
        } else {
            None
        }
    }
}
