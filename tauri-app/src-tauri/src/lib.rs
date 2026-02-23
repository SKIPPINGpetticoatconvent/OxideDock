mod config;
mod icon_extractor;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use std::sync::Mutex;
use tauri::{Manager, State, WindowEvent};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, SW_HIDE, SW_SHOW, ShowWindow};
use windows::core::w;

pub struct AppState {
    pub config: config::Config,
}

#[tauri::command]
fn get_config(state: State<'_, Mutex<AppState>>) -> Result<serde_json::Value, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    serde_json::to_value(&state.config).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_icon_base64(path: String) -> Result<Option<String>, String> {
    if let Some(img) = icon_extractor::extract_icon(&path) {
        let (w, h) = (img.width(), img.height());
        let mut png_bytes: Vec<u8> = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
        if encoder.encode(&img, w, h, image::ColorType::Rgba8).is_ok() {
            return Ok(Some(format!(
                "data:image/png;base64,{}",
                BASE64.encode(&png_bytes)
            )));
        }
    }
    Ok(None)
}

fn hide_taskbar() {
    unsafe {
        let taskbar_hwnd = FindWindowW(w!("Shell_TrayWnd"), None);
        if taskbar_hwnd != HWND::default() {
            ShowWindow(taskbar_hwnd, SW_HIDE);
        }
    }
}

fn show_taskbar() {
    unsafe {
        let taskbar_hwnd = FindWindowW(w!("Shell_TrayWnd"), None);
        if taskbar_hwnd != HWND::default() {
            ShowWindow(taskbar_hwnd, SW_SHOW);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config = config::load_config("config.json")
        .unwrap_or_else(|_| config::Config { categories: vec![] });

    tauri::Builder::default()
        .manage(Mutex::new(AppState { config }))
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![get_config, get_icon_base64])
        .setup(|app| {
            // Hide taskbar on startup
            hide_taskbar();

            // Listen for window close to restore taskbar
            let main_window = app.get_webview_window("main").unwrap();
            main_window.on_window_event(move |event| {
                if let WindowEvent::Destroyed = event {
                    show_taskbar();
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
