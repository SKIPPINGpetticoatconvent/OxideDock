use crate::config::Config;
use crate::icon_extractor::SystemIcon;
use std::sync::{Mutex, OnceLock};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, EndPaint, FillRect, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DI_NORMAL, DefWindowProcW,
    DispatchMessageW, DrawIconEx, GetMessageW, MSG, PostQuitMessage, RegisterClassExW, SW_HIDE,
    SW_SHOW, ShowWindow, TranslateMessage, WM_DESTROY, WM_LBUTTONUP, WM_PAINT, WNDCLASSEXW,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};
use windows::core::{Error, w};

pub struct AppState {
    pub config: Config,
    pub icons: Vec<Option<SystemIcon>>,
}

static STATE: OnceLock<Mutex<AppState>> = OnceLock::new();

pub fn run(config: Config) -> Result<(), Error> {
    let mut icons = Vec::new();
    for cat in &config.categories {
        for sc in &cat.shortcuts {
            icons.push(crate::icon_extractor::extract_icon(&sc.path));
        }
    }

    let state = AppState { config, icons };
    let _ = STATE.set(Mutex::new(state));

    unsafe {
        let instance = GetModuleHandleW(None)?;
        debug_assert!(instance.0 != 0);

        let window_class = w!("OxideDockWindow");

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: window_class,
            hbrBackground: CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x001A_1A1A)),
            ..Default::default()
        };

        let atom = RegisterClassExW(&wc);
        debug_assert!(atom != 0);

        let size = 300;

        let _hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            window_class,
            w!("OxideDock"),
            WS_POPUP | WS_VISIBLE,
            100,
            100,
            size,
            size, // Fixed size for MVP
            None,
            None,
            instance,
            None,
        );

        let mut message = MSG::default();

        while GetMessageW(&mut message, None, 0, 0).into() {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        Ok(())
    }
}

extern "system" fn wndproc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match message {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(window, &mut ps);

                let bg = CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x002B_2B2B));
                FillRect(hdc, &ps.rcPaint, bg);

                if let Some(mutex) = STATE.get() {
                    if let Ok(state) = mutex.lock() {
                        let mut x = 20;
                        let mut y = 20;
                        for icon_opt in &state.icons {
                            if let Some(sys_icon) = icon_opt {
                                DrawIconEx(hdc, x, y, sys_icon.hicon, 64, 64, 0, None, DI_NORMAL);
                            }
                            x += 80;
                            if x >= 240 {
                                x = 20;
                                y += 80;
                            }
                        }
                    }
                }

                EndPaint(window, &ps);
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let x = (lparam.0 & 0xFFFF) as i32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as i32;

                // Simple hit testing for MVP (80x80 grid cells, 64x64 icons)
                let col = (x - 20) / 80;
                let row = (y - 20) / 80;

                if col >= 0 && col < 3 && row >= 0 && row < 3 {
                    let index = (row * 3 + col) as usize;
                    if let Some(mutex) = STATE.get() {
                        if let Ok(state) = mutex.lock() {
                            let mut current_idx = 0;
                            let mut found_path = None;
                            for cat in &state.config.categories {
                                for sc in &cat.shortcuts {
                                    if current_idx == index {
                                        found_path = Some(sc.path.clone());
                                    }
                                    current_idx += 1;
                                }
                            }

                            if let Some(path) = found_path {
                                println!("Launching: {}", path);
                                let _ = std::process::Command::new(path).spawn();
                                ShowWindow(window, SW_HIDE);
                            }
                        }
                    }
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(window, message, wparam, lparam),
        }
    }
}
