use image::RgbaImage;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDIBits, SelectObject,
};
use windows::Win32::UI::Controls::IImageList;
use windows::Win32::UI::Shell::{
    SHFILEINFOW, SHGFI_SYSICONINDEX, SHGetFileInfoW, SHGetImageList, SHIL_EXTRALARGE, SHIL_JUMBO,
};
use windows::Win32::UI::WindowsAndMessaging::DestroyIcon;
use windows::core::PCWSTR;

/// Extract the highest-resolution icon for a given file path.
/// Uses SHGetImageList(SHIL_JUMBO) to get 256×256 icons on modern Windows,
/// falling back to SHIL_EXTRALARGE (48×48) and then to windows-icons crate.
pub fn extract_icon(path: &str) -> Option<RgbaImage> {
    // Verify path exists
    if !Path::new(path).exists() {
        println!("  Icon FAIL: '{}' -> file does not exist", path);
        return None;
    }

    // Try JUMBO first (256x256), then EXTRALARGE (48x48)
    if let Some(img) = extract_shell_icon(path, SHIL_JUMBO as i32) {
        println!(
            "  Icon OK: '{}' ({}x{}) [JUMBO]",
            path,
            img.width(),
            img.height()
        );
        return Some(img);
    }

    if let Some(img) = extract_shell_icon(path, SHIL_EXTRALARGE as i32) {
        println!(
            "  Icon OK: '{}' ({}x{}) [EXTRALARGE]",
            path,
            img.width(),
            img.height()
        );
        return Some(img);
    }

    // Final fallback: windows-icons crate
    match windows_icons::get_icon_by_path(path) {
        Ok(icon) => {
            let (w, h) = (icon.width(), icon.height());
            println!("  Icon OK: '{}' ({}x{}) [fallback]", path, w, h);
            if w < 48 {
                Some(image::imageops::resize(
                    &icon,
                    128,
                    128,
                    image::imageops::FilterType::Lanczos3,
                ))
            } else {
                Some(icon)
            }
        }
        Err(e) => {
            println!("  Icon FAIL: '{}' -> {}", path, e);
            None
        }
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn extract_shell_icon(path: &str, image_list_type: i32) -> Option<RgbaImage> {
    unsafe {
        // Get the icon index in the system image list
        let wide_path = to_wide(path);
        let mut shfi = SHFILEINFOW::default();
        let result = SHGetFileInfoW(
            PCWSTR(wide_path.as_ptr()),
            windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL,
            Some(&mut shfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_SYSICONINDEX,
        );

        if result == 0 {
            return None;
        }

        let icon_index = shfi.iIcon;

        // Get the image list for the requested size
        let image_list: IImageList = SHGetImageList(image_list_type).ok()?;

        // Get the icon size
        let mut size = SIZE::default();
        image_list.GetIconSize(&mut size.cx, &mut size.cy).ok()?;
        let icon_w = size.cx as u32;
        let icon_h = size.cy as u32;

        if icon_w == 0 || icon_h == 0 {
            return None;
        }

        // Extract HICON
        let hicon = image_list.GetIcon(icon_index, 0).ok()?;

        // Convert HICON to RGBA pixels
        let img = hicon_to_rgba(hicon, icon_w, icon_h);

        // Cleanup
        let _ = DestroyIcon(hicon);

        img
    }
}

unsafe fn hicon_to_rgba(
    hicon: windows::Win32::UI::WindowsAndMessaging::HICON,
    width: u32,
    height: u32,
) -> Option<RgbaImage> {
    use windows::Win32::Graphics::Gdi::CreateDIBSection;
    use windows::Win32::UI::WindowsAndMessaging::{GetIconInfo, ICONINFO};

    // Get icon info to access the bitmaps
    let mut icon_info = ICONINFO::default();
    if !GetIconInfo(hicon, &mut icon_info).is_ok() {
        return None;
    }

    // Create a memory DC
    let hdc_screen =
        windows::Win32::Graphics::Gdi::GetDC(windows::Win32::Foundation::HWND::default());
    let hdc = CreateCompatibleDC(hdc_screen);

    // Setup BITMAPINFO for 32-bit BGRA
    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32), // Top-down DIB
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [Default::default()],
    };

    // Create a DIB section to render the icon into
    let mut bits_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
    let hbm = CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &mut bits_ptr, None, 0).ok()?;

    let old_bm = SelectObject(hdc, hbm);

    // Draw the icon into our DIB
    windows::Win32::UI::WindowsAndMessaging::DrawIconEx(
        hdc,
        0,
        0,
        hicon,
        width as i32,
        height as i32,
        0,
        None,
        windows::Win32::UI::WindowsAndMessaging::DI_NORMAL,
    )
    .ok()?;

    // Read the pixel data
    let pixel_count = (width * height) as usize;
    let mut pixels = vec![0u8; pixel_count * 4];

    GetDIBits(
        hdc,
        hbm,
        0,
        height,
        Some(pixels.as_mut_ptr() as *mut _),
        &mut bmi,
        DIB_RGB_COLORS,
    );

    // Convert BGRA → RGBA
    for i in (0..pixels.len()).step_by(4) {
        pixels.swap(i, i + 2); // Swap B and R
    }

    // Cleanup GDI objects
    SelectObject(hdc, old_bm);
    DeleteObject(hbm);
    DeleteDC(hdc);
    windows::Win32::Graphics::Gdi::ReleaseDC(
        windows::Win32::Foundation::HWND::default(),
        hdc_screen,
    );

    if !icon_info.hbmColor.is_invalid() {
        DeleteObject(icon_info.hbmColor);
    }
    if !icon_info.hbmMask.is_invalid() {
        DeleteObject(icon_info.hbmMask);
    }

    // Create image, skip if entirely transparent
    let img = RgbaImage::from_raw(width, height, pixels)?;

    // Check if image has any non-zero alpha (not blank)
    let has_content = img.pixels().any(|p| p.0[3] > 0);
    if has_content {
        Some(img)
    } else {
        None // Blank icon, try next method
    }
}
