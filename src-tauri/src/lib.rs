mod config;
mod icon_extractor;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use image::ImageEncoder;
use std::sync::Mutex;
use tauri::{Manager, State, WindowEvent};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::Shell::{
    ABE_BOTTOM, ABM_NEW, ABM_REMOVE, ABM_SETPOS, APPBARDATA, SHAppBarMessage,
};
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
        if encoder
            .write_image(&img, w, h, image::ExtendedColorType::Rgba8)
            .is_ok()
        {
            return Ok(Some(format!(
                "data:image/png;base64,{}",
                BASE64.encode(&png_bytes)
            )));
        }
    }
    Ok(None)
}

#[tauri::command]
fn launch_app(path: String) -> Result<(), String> {
    std::process::Command::new(&path)
        .spawn()
        .map_err(|e| format!("Failed to launch {}: {}", path, e))?;
    Ok(())
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

// ── AppBar: reserve screen space so maximized windows don't cover the dock ──

fn register_appbar(hwnd: HWND, dock_height: i32, screen_width: i32, screen_height: i32) {
    unsafe {
        let mut abd = APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            hWnd: hwnd,
            ..Default::default()
        };

        // Register as an AppBar
        let result = SHAppBarMessage(ABM_NEW, &mut abd);
        if result == 0 {
            eprintln!("AppBar: ABM_NEW failed");
            return;
        }
        println!("AppBar: Registered successfully");

        // Set position at screen bottom
        abd.uEdge = ABE_BOTTOM as u32;
        abd.rc = RECT {
            left: 0,
            top: screen_height - dock_height,
            right: screen_width,
            bottom: screen_height,
        };

        SHAppBarMessage(ABM_SETPOS, &mut abd);
        println!(
            "AppBar: Reserved bottom {}px (top={}, bottom={})",
            dock_height, abd.rc.top, abd.rc.bottom
        );
    }
}

fn unregister_appbar(hwnd: HWND) {
    unsafe {
        let mut abd = APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            hWnd: hwnd,
            ..Default::default()
        };
        SHAppBarMessage(ABM_REMOVE, &mut abd);
        println!("AppBar: Unregistered");
    }
}

fn find_config() -> std::path::PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));

    let candidates = [
        exe_dir.as_ref().map(|d| d.join("config.json")),
        Some(std::path::PathBuf::from("config.json")),
        Some(std::path::PathBuf::from("src-tauri/config.json")),
    ];

    for candidate in candidates.iter().flatten() {
        if candidate.exists() {
            println!("Found config at: {:?}", candidate);
            return candidate.clone();
        }
    }

    std::path::PathBuf::from("config.json")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config_path = find_config();
    let config = config::load_config(&config_path).unwrap_or_else(|e| {
        eprintln!("Failed to load config from {:?}: {}", config_path, e);
        config::Config { categories: vec![] }
    });

    println!("Config loaded: {} categories", config.categories.len());

    tauri::Builder::default()
        .manage(Mutex::new(AppState { config }))
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_config,
            get_icon_base64,
            launch_app
        ])
        .setup(|app| {
            // Hide the native taskbar
            hide_taskbar();

            let main_window = app.get_webview_window("main").unwrap();

            // Get screen dimensions for positioning
            if let Some(monitor) = main_window.current_monitor().ok().flatten() {
                let screen = monitor.size();
                let scale = monitor.scale_factor();
                let logical_h = (screen.height as f64 / scale) as i32;
                let logical_w = (screen.width as f64 / scale) as i32;
                let dock_height = 90;

                // Set dock window size and position
                let _ = main_window.set_size(tauri::LogicalSize::new(logical_w, dock_height));
                let _ = main_window
                    .set_position(tauri::LogicalPosition::new(0, logical_h - dock_height));

                // Register AppBar to reserve screen bottom space
                // Use physical pixels for AppBar (Windows API expects physical)
                let phys_w = screen.width as i32;
                let phys_h = screen.height as i32;
                let phys_dock_h = (dock_height as f64 * scale) as i32;

                // Get native HWND from Tauri window
                #[cfg(target_os = "windows")]
                {
                    if let Ok(hwnd_raw) = main_window.hwnd() {
                        let hwnd = HWND(hwnd_raw.0 as isize);
                        register_appbar(hwnd, phys_dock_h, phys_w, phys_h);

                        // Store HWND for cleanup on close
                        main_window.on_window_event(move |event| {
                            if let WindowEvent::Destroyed = event {
                                unregister_appbar(hwnd);
                                show_taskbar();
                            }
                        });
                    }
                }
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
