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
    pub is_hidden: bool,
}

#[tauri::command]
fn get_config(state: State<'_, Mutex<AppState>>) -> Result<serde_json::Value, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    serde_json::to_value(&state.config).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_dock_hidden(
    window: tauri::WebviewWindow,
    state: State<'_, Mutex<AppState>>,
    hidden: bool,
) -> Result<(), String> {
    {
        let mut state = state.lock().map_err(|e| e.to_string())?;
        state.is_hidden = hidden;
    }
    update_dock_position(&window, &state);
    Ok(())
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

#[tauri::command]
fn get_running_apps() -> Result<Vec<String>, String> {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::ProcessStatus::EnumProcesses;
        use windows::Win32::System::Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        };

        let mut pids = [0u32; 1024];
        let mut cb_needed = 0u32;
        unsafe {
            if EnumProcesses(pids.as_mut_ptr(), (pids.len() as u32) * 4, &mut cb_needed).is_ok() {
                let count = (cb_needed / 4) as usize;
                let mut paths = std::collections::HashSet::new();

                for i in 0..count {
                    let pid = pids[i];
                    if pid == 0 {
                        continue;
                    }

                    if let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                        let mut buffer = [0u16; 1024];
                        let mut size = buffer.len() as u32;
                        if QueryFullProcessImageNameW(
                            handle,
                            PROCESS_NAME_WIN32,
                            windows::core::PWSTR(buffer.as_mut_ptr()),
                            &mut size,
                        )
                        .is_ok()
                        {
                            let path = String::from_utf16_lossy(&buffer[..size as usize]);
                            paths.insert(path.to_lowercase());
                        }
                        let _ = CloseHandle(handle);
                    }
                }
                return Ok(paths.into_iter().collect());
            }
        }
    }
    Ok(vec![])
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

// ─── Positioning and AppBar logic ───

fn update_dock_position(window: &tauri::WebviewWindow, state_mutex: &Mutex<AppState>) {
    let is_hidden = state_mutex.lock().map(|s| s.is_hidden).unwrap_or(false);

    if let Some(monitor) = window.current_monitor().ok().flatten() {
        let screen_size = monitor.size();
        let scale = monitor.scale_factor();
        let monitor_pos = monitor.position();

        let logical_dock_height = 82; // Optimized Height
        let phys_dock_h = (logical_dock_height as f64 * scale).round() as i32;

        let phys_bottom_y = if is_hidden {
            // Hidden: Only 4 pixels visible
            monitor_pos.y + screen_size.height as i32 - 4
        } else {
            monitor_pos.y + screen_size.height as i32 - phys_dock_h
        };

        let phys_left_x = monitor_pos.x;

        // Apply window size and position (Physical)
        let _ = window.set_size(tauri::PhysicalSize::new(
            screen_size.width,
            phys_dock_h as u32,
        ));
        let _ = window.set_position(tauri::PhysicalPosition::new(phys_left_x, phys_bottom_y));

        #[cfg(target_os = "windows")]
        {
            if let Ok(hwnd_raw) = window.hwnd() {
                let hwnd = HWND(hwnd_raw.0 as isize);
                unregister_appbar(hwnd); // Clear previous area

                if !is_hidden {
                    register_appbar(
                        hwnd,
                        phys_dock_h,
                        screen_size.width as i32,
                        screen_size.height as i32,
                    );
                }
            }
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
        .manage(Mutex::new(AppState {
            config,
            is_hidden: false,
        }))
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_config,
            get_icon_base64,
            launch_app,
            get_running_apps,
            set_dock_hidden
        ])
        .setup(|app| {
            hide_taskbar();

            let main_window = app.get_webview_window("main").unwrap();
            let state = app.state::<Mutex<AppState>>();

            // Initial positioning
            update_dock_position(&main_window, &state);

            // Listen for changes to handle resolution/scaling automatically
            let window_ref = main_window.clone();
            let state_ref = state.inner().clone();
            main_window.on_window_event(move |event| match event {
                WindowEvent::ScaleFactorChanged { .. }
                | WindowEvent::Moved { .. }
                | WindowEvent::Resized(..) => {
                    update_dock_position(&window_ref, state_ref);
                }
                WindowEvent::Destroyed => {
                    #[cfg(target_os = "windows")]
                    {
                        if let Ok(hwnd_raw) = window_ref.hwnd() {
                            unregister_appbar(HWND(hwnd_raw.0 as isize));
                        }
                    }
                    show_taskbar();
                }
                _ => {}
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
