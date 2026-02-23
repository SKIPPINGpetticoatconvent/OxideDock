use image::RgbaImage;

/// Extract icon from a file path as an RgbaImage.
/// Uses the windows-icons crate for reliable icon extraction.
pub fn extract_icon(path: &str) -> Option<RgbaImage> {
    match windows_icons::get_icon_by_path(path) {
        Ok(icon) => {
            println!("  Icon OK: '{}' ({}x{})", path, icon.width(), icon.height());
            Some(icon)
        }
        Err(e) => {
            println!("  Icon FAIL: '{}' -> {}", path, e);
            None
        }
    }
}
