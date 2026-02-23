use std::ptr::null_mut;
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::UI::Controls::IImageList;
use windows::Win32::UI::Shell::{
    SHFILEINFOW, SHGFI_SYSICONINDEX, SHGetFileInfoW, SHGetImageList, SHIL_JUMBO,
};
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
            SHGFI_SYSICONINDEX,
        );

        if result == 0 {
            return None;
        }

        let i_icon = shfi.iIcon;

        match SHGetImageList::<IImageList>(SHIL_JUMBO as i32) {
            Ok(list) => {
                // GetIcon returns a Result<HICON> in windows-rs 0.52+
                if let Ok(hicon) = list.GetIcon(i_icon, 0u32) {
                    return Some(SystemIcon { hicon });
                }
                None
            }
            Err(_) => None,
        }
    }
}
