use crate::config::Config;
use crate::icon_extractor::SystemIcon;
use std::sync::{Mutex, OnceLock};

use windows::Foundation::Numerics::{Vector2, Vector3};
use windows::Foundation::TimeSpan;
use windows::UI::Composition::{
    Compositor, ContainerVisual, Desktop::DesktopWindowTarget, SpriteVisual,
    Vector3KeyFrameAnimation,
};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_PROPERTIES, ID2D1DeviceContext,
};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::WinRT::Composition::{
    ICompositionDrawingSurfaceInterop, ICompositorDesktopInterop, ICompositorInterop,
};
use windows::Win32::System::WinRT::{
    CreateDispatcherQueueController, DQTAT_COM_ASTA, DQTYPE_THREAD_CURRENT, DispatcherQueueOptions,
    RO_INIT_SINGLETHREADED, RoInitialize,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DI_NORMAL, DefWindowProcW, DispatchMessageW,
    DrawIconEx, FindWindowW, GetMessageW, GetSystemMetrics, IDC_ARROW, LoadCursorW, MSG,
    PostQuitMessage, RegisterClassExW, SM_CXSCREEN, SM_CYSCREEN, SW_HIDE, SW_SHOW, ShowWindow,
    TranslateMessage, WM_DESTROY, WM_LBUTTONUP, WM_MOUSEMOVE, WNDCLASSEXW,
    WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};
use windows::core::{ComInterface, HSTRING, Result, w};

// ═══════════════════════════════════════════════════════════════
// macOS Dock Light Mode - Visual Constants
// ═══════════════════════════════════════════════════════════════
const ICON_SIZE: f32 = 48.0; // Default icon size (px)
const ICON_SPACING: f32 = 10.0; // Gap between icons
const ICON_SLOT: f32 = ICON_SIZE + ICON_SPACING; // Total slot width per icon
const BAR_PADDING_H: f32 = 12.0; // Horizontal padding inside dock bar
const BAR_PADDING_V: f32 = 6.0; // Vertical padding inside dock bar
const BAR_HEIGHT: f32 = ICON_SIZE + BAR_PADDING_V * 2.0; // Total bar height
const BAR_CORNER_RADIUS: f32 = 16.0; // Rounded corner radius
const BAR_BOTTOM_MARGIN: f32 = 8.0; // Gap between dock bottom and screen bottom
const MAX_SCALE: f32 = 1.8; // Maximum magnification factor
const SIGMA: f32 = 90.0; // Gaussian spread for magnification
const ANIM_DURATION: i64 = 800000; // 80ms animation duration (100ns units)

// macOS Light Mode Colors
const BG_ALPHA: u8 = 180; // ~70% opacity
const BG_R: u8 = 240;
const BG_G: u8 = 240;
const BG_B: u8 = 240;

pub struct AppState {
    pub config: Config,
    pub icons: Vec<Option<SystemIcon>>,
    pub compositor: Option<Compositor>,
    pub target: Option<DesktopWindowTarget>,
    pub root_visual: Option<ContainerVisual>,
    pub icon_visuals: Vec<SpriteVisual>,
    pub bg_bar_visual: Option<SpriteVisual>,
    pub base_x_positions: Vec<f32>,
    pub scale_animations: Vec<Vector3KeyFrameAnimation>,
    pub offset_animations: Vec<Vector3KeyFrameAnimation>,
    pub win_width: i32,
    pub win_height: i32,
    pub taskbar_hwnd: Option<HWND>,
}

static STATE: OnceLock<Mutex<AppState>> = OnceLock::new();

pub fn run(config: Config) -> Result<()> {
    unsafe {
        RoInitialize(RO_INIT_SINGLETHREADED)?;
        let options = DispatcherQueueOptions {
            dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_ASTA,
        };
        let _controller = CreateDispatcherQueueController(options)?;
        let compositor = Compositor::new()?;

        // Use full physical screen to overlay taskbar
        let screen_width = GetSystemMetrics(SM_CXSCREEN);
        let screen_height = GetSystemMetrics(SM_CYSCREEN);
        let win_width = screen_width;
        let win_height = 200; // Enough room for magnified icons
        let win_x = 0;
        let win_y = screen_height - win_height;

        // Extract all icons
        let mut icons_raw = Vec::new();
        for cat in &config.categories {
            for sc in &cat.shortcuts {
                icons_raw.push(crate::icon_extractor::extract_icon(&sc.path));
            }
        }

        let state_obj = AppState {
            config,
            icons: icons_raw,
            compositor: Some(compositor.clone()),
            target: None,
            root_visual: None,
            icon_visuals: Vec::new(),
            bg_bar_visual: None,
            base_x_positions: Vec::new(),
            scale_animations: Vec::new(),
            offset_animations: Vec::new(),
            win_width,
            win_height,
            taskbar_hwnd: None,
        };
        let _ = STATE.set(Mutex::new(state_obj));

        let instance = GetModuleHandleW(None)?;
        let window_class = w!("OxideDockWindow");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: window_class,
            ..Default::default()
        };
        RegisterClassExW(&wc);

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOREDIRECTIONBITMAP,
            window_class,
            w!("OxideDock"),
            WS_POPUP | WS_VISIBLE,
            win_x,
            win_y,
            win_width,
            win_height,
            None,
            None,
            instance,
            None,
        );

        // Setup Composition
        let interop: ICompositorDesktopInterop = compositor.cast()?;
        let target = interop.CreateDesktopWindowTarget(hwnd, true)?;
        let root_visual = compositor.CreateContainerVisual()?;
        target.SetRoot(&root_visual)?;

        // ═══ Background Bar (macOS Light Mode: milky white translucent) ═══
        let bg_bar_visual = compositor.CreateSpriteVisual()?;
        let bg_brush = compositor.CreateColorBrushWithColor(windows::UI::Color {
            A: BG_ALPHA,
            R: BG_R,
            G: BG_G,
            B: BG_B,
        })?;
        bg_bar_visual.SetBrush(&bg_brush)?;

        // Round corners via GeometricClip
        let rounded_rect = compositor.CreateRoundedRectangleGeometry()?;
        rounded_rect.SetCornerRadius(Vector2 {
            X: BAR_CORNER_RADIUS,
            Y: BAR_CORNER_RADIUS,
        })?;
        // Size will be updated after we know icon count
        let clip = compositor.CreateGeometricClip()?;
        clip.SetGeometry(&rounded_rect)?;
        bg_bar_visual.SetClip(&clip)?;

        root_visual.Children()?.InsertAtBottom(&bg_bar_visual)?;

        // Setup D3D/D2D for icon rendering
        let mut d3d_device: Option<ID3D11Device> = None;
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            None,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut d3d_device),
            None,
            None,
        )?;
        let d3d_device = d3d_device.unwrap();
        let dxgi_device: IDXGIDevice = d3d_device.cast()?;
        let interop2: ICompositorInterop = compositor.cast()?;
        let comp_graphics_dev = interop2.CreateGraphicsDevice(&dxgi_device)?;

        let n_icons = {
            let mutex = STATE.get().unwrap();
            let state = mutex.lock().unwrap();
            state.icons.len()
        };

        // Calculate centered position for all icons
        let total_icons_width = n_icons as f32 * ICON_SLOT - ICON_SPACING; // No trailing space
        let bar_total_width = total_icons_width + BAR_PADDING_H * 2.0;
        let bar_x = (win_width as f32 - bar_total_width) / 2.0;
        // Y position: bar sits at bottom of window minus margin
        let bar_y = win_height as f32 - BAR_HEIGHT - BAR_BOTTOM_MARGIN;
        let icons_start_x = bar_x + BAR_PADDING_H;
        let icon_y = bar_y + BAR_PADDING_V;

        // Set initial background bar
        let _ = bg_bar_visual.SetSize(Vector2 {
            X: bar_total_width,
            Y: BAR_HEIGHT,
        });
        let _ = bg_bar_visual.SetOffset(Vector3 {
            X: bar_x,
            Y: bar_y,
            Z: 0.0,
        });
        let _ = rounded_rect.SetSize(Vector2 {
            X: bar_total_width,
            Y: BAR_HEIGHT,
        });

        let mut current_x = icons_start_x;

        if let Some(mutex) = STATE.get() {
            if let Ok(mut state) = mutex.lock() {
                state.bg_bar_visual = Some(bg_bar_visual.clone());
                let icons_copy = state.icons.clone();
                for icon_opt in icons_copy {
                    if let Some(sys_icon) = icon_opt {
                        let icon_visual = compositor.CreateSpriteVisual()?;
                        icon_visual.SetSize(Vector2 {
                            X: ICON_SIZE,
                            Y: ICON_SIZE,
                        })?;
                        icon_visual.SetOffset(Vector3 {
                            X: current_x,
                            Y: icon_y,
                            Z: 0.0,
                        })?;
                        // CenterPoint at bottom-center for macOS-style "grow upward" effect
                        icon_visual.SetCenterPoint(Vector3 {
                            X: ICON_SIZE / 2.0,
                            Y: ICON_SIZE,
                            Z: 0.0,
                        })?;

                        let scale_anim = compositor.CreateVector3KeyFrameAnimation()?;
                        let _ = scale_anim.SetDuration(TimeSpan {
                            Duration: ANIM_DURATION,
                        });
                        let offset_anim = compositor.CreateVector3KeyFrameAnimation()?;
                        let _ = offset_anim.SetDuration(TimeSpan {
                            Duration: ANIM_DURATION,
                        });

                        // Render icon to Composition surface
                        let comp_surface = comp_graphics_dev.CreateDrawingSurface(
                            windows::Foundation::Size {
                                Width: ICON_SIZE,
                                Height: ICON_SIZE,
                            },
                            windows::Graphics::DirectX::DirectXPixelFormat::B8G8R8A8UIntNormalized,
                            windows::Graphics::DirectX::DirectXAlphaMode::Premultiplied,
                        )?;
                        let surface_interop: ICompositionDrawingSurfaceInterop =
                            comp_surface.cast()?;
                        let mut update_offset = POINT::default();
                        let d2d_ctx: ID2D1DeviceContext =
                            surface_interop.BeginDraw(None, &mut update_offset)?;

                        {
                            let sz = ICON_SIZE as i32;
                            let mut bmi = BITMAPINFO::default();
                            bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
                            bmi.bmiHeader.biWidth = sz;
                            bmi.bmiHeader.biHeight = -sz;
                            bmi.bmiHeader.biPlanes = 1;
                            bmi.bmiHeader.biBitCount = 32;
                            bmi.bmiHeader.biCompression = BI_RGB.0;
                            let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
                            let hdc_mem = CreateCompatibleDC(None);
                            let hbitmap = CreateDIBSection(
                                hdc_mem,
                                &bmi,
                                DIB_RGB_COLORS,
                                &mut bits,
                                None,
                                0,
                            )?;
                            SelectObject(hdc_mem, hbitmap);
                            let _ = DrawIconEx(
                                hdc_mem,
                                0,
                                0,
                                sys_icon.hicon,
                                sz,
                                sz,
                                0,
                                None,
                                DI_NORMAL,
                            );
                            let props = D2D1_BITMAP_PROPERTIES {
                                pixelFormat: D2D1_PIXEL_FORMAT {
                                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                                },
                                dpiX: 96.0,
                                dpiY: 96.0,
                            };
                            let d2d_bitmap = d2d_ctx.CreateBitmap(
                                D2D_SIZE_U {
                                    width: sz as u32,
                                    height: sz as u32,
                                },
                                Some(bits),
                                sz as u32 * 4,
                                &props,
                            )?;
                            d2d_ctx.BeginDraw();
                            d2d_ctx.Clear(None);
                            let dest_rect = D2D_RECT_F {
                                left: update_offset.x as f32,
                                top: update_offset.y as f32,
                                right: (update_offset.x + sz) as f32,
                                bottom: (update_offset.y + sz) as f32,
                            };
                            d2d_ctx.DrawBitmap(
                                &d2d_bitmap,
                                Some(&dest_rect),
                                1.0,
                                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                                None,
                            );
                            let _ = d2d_ctx.EndDraw(None, None);
                            DeleteDC(hdc_mem);
                            DeleteObject(hbitmap);
                        }
                        surface_interop.EndDraw()?;

                        let brush = compositor.CreateSurfaceBrushWithSurface(&comp_surface)?;
                        icon_visual.SetBrush(&brush)?;
                        root_visual.Children()?.InsertAtTop(&icon_visual)?;

                        state.icon_visuals.push(icon_visual);
                        state.base_x_positions.push(current_x);
                        state.scale_animations.push(scale_anim);
                        state.offset_animations.push(offset_anim);
                    }
                    current_x += ICON_SLOT;
                }
            }
        }

        // Hide Windows taskbar
        let taskbar_hwnd = FindWindowW(w!("Shell_TrayWnd"), None);
        if taskbar_hwnd != HWND::default() {
            ShowWindow(taskbar_hwnd, SW_HIDE);
        }

        ShowWindow(hwnd, SW_SHOW);
        if let Some(mutex) = STATE.get() {
            if let Ok(mut state) = mutex.lock() {
                state.target = Some(target);
                state.root_visual = Some(root_visual);
                if taskbar_hwnd != HWND::default() {
                    state.taskbar_hwnd = Some(taskbar_hwnd);
                }
            }
        }

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).into() {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════
// Dock layout calculation helpers
// ═══════════════════════════════════════════════════════════════

/// Calculate dock layout (scales, positions) based on mouse_x
/// Returns (scales, x_positions, bar_width, bar_x, bar_y)
fn calc_dock_layout(
    base_positions: &[f32],
    mouse_x: f32,
    win_width: i32,
    win_height: i32,
    magnify: bool,
) -> (Vec<f32>, Vec<Vector3>, f32, f32, f32) {
    let n = base_positions.len();
    let mut scales = vec![1.0f32; n];
    let mut icon_widths = vec![ICON_SIZE; n];

    if magnify {
        for i in 0..n {
            let center_x = base_positions[i] + ICON_SIZE / 2.0;
            let dist = (mouse_x - center_x).abs();
            let m = MAX_SCALE - 1.0;
            scales[i] = 1.0 + m * (-(dist * dist) / (2.0 * SIGMA * SIGMA)).exp();
            icon_widths[i] = scales[i] * ICON_SIZE;
        }
    }

    // Total width including scaled icons + spacing
    let total_width: f32 = icon_widths.iter().sum::<f32>() + (n as f32 - 1.0) * ICON_SPACING;
    let bar_width = total_width + BAR_PADDING_H * 2.0;
    let bar_x = (win_width as f32 - bar_width) / 2.0;
    let bar_y = win_height as f32 - BAR_HEIGHT - BAR_BOTTOM_MARGIN;

    // Position each icon centered within the bar
    let mut positions = Vec::with_capacity(n);
    let mut current_x = bar_x + BAR_PADDING_H;
    for i in 0..n {
        // Icons "grow upward" from the bar bottom
        let scaled_height = scales[i] * ICON_SIZE;
        let icon_y = bar_y + BAR_HEIGHT - BAR_PADDING_V - scaled_height;
        positions.push(Vector3 {
            X: current_x,
            Y: icon_y,
            Z: 0.0,
        });
        current_x += icon_widths[i] + ICON_SPACING;
    }

    (scales, positions, bar_width, bar_x, bar_y)
}

extern "system" fn wndproc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match message {
            WM_MOUSEMOVE => {
                let mouse_x = (lparam.0 & 0xFFFF) as i32 as f32;

                // Enable WM_MOUSELEAVE tracking
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: window,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut tme);

                if let Some(mutex) = STATE.get() {
                    if let Ok(state) = mutex.lock() {
                        let n = state.icon_visuals.len();
                        if n == 0 {
                            return LRESULT(0);
                        }

                        let (scales, positions, bar_width, bar_x, bar_y) = calc_dock_layout(
                            &state.base_x_positions,
                            mouse_x,
                            state.win_width,
                            state.win_height,
                            true,
                        );

                        for i in 0..n {
                            let visual = &state.icon_visuals[i];
                            let scale_anim = &state.scale_animations[i];
                            let offset_anim = &state.offset_animations[i];

                            let _ = scale_anim.InsertKeyFrame(
                                1.0,
                                Vector3 {
                                    X: scales[i],
                                    Y: scales[i],
                                    Z: 1.0,
                                },
                            );
                            let _ = visual.StartAnimation(&HSTRING::from("Scale"), scale_anim);

                            let _ = offset_anim.InsertKeyFrame(1.0, positions[i]);
                            let _ = visual.StartAnimation(&HSTRING::from("Offset"), offset_anim);
                        }

                        // Animate background bar
                        if let Some(bg) = &state.bg_bar_visual {
                            let _ = bg.SetSize(Vector2 {
                                X: bar_width,
                                Y: BAR_HEIGHT,
                            });
                            let _ = bg.SetOffset(Vector3 {
                                X: bar_x,
                                Y: bar_y,
                                Z: 0.0,
                            });
                            // Update clip geometry for rounded corners
                            if let Ok(clip) = bg.Clip() {
                                let geo_clip: windows::UI::Composition::CompositionGeometricClip =
                                    clip.cast().unwrap();
                                if let Ok(geo) = geo_clip.Geometry() {
                                    let rounded: windows::UI::Composition::CompositionRoundedRectangleGeometry = geo.cast().unwrap();
                                    let _ = rounded.SetSize(Vector2 {
                                        X: bar_width,
                                        Y: BAR_HEIGHT,
                                    });
                                }
                            }
                        }
                    }
                }
                LRESULT(0)
            }
            0x02A3 => {
                // WM_MOUSELEAVE: reset to default layout
                if let Some(mutex) = STATE.get() {
                    if let Ok(state) = mutex.lock() {
                        let n = state.icon_visuals.len();
                        if n == 0 {
                            return LRESULT(0);
                        }

                        let (_, positions, bar_width, bar_x, bar_y) = calc_dock_layout(
                            &state.base_x_positions,
                            0.0,
                            state.win_width,
                            state.win_height,
                            false,
                        );

                        for i in 0..n {
                            let visual = &state.icon_visuals[i];
                            let scale_anim = &state.scale_animations[i];
                            let offset_anim = &state.offset_animations[i];

                            let _ = scale_anim.InsertKeyFrame(
                                1.0,
                                Vector3 {
                                    X: 1.0,
                                    Y: 1.0,
                                    Z: 1.0,
                                },
                            );
                            let _ = visual.StartAnimation(&HSTRING::from("Scale"), scale_anim);

                            let _ = offset_anim.InsertKeyFrame(1.0, positions[i]);
                            let _ = visual.StartAnimation(&HSTRING::from("Offset"), offset_anim);
                        }

                        if let Some(bg) = &state.bg_bar_visual {
                            let _ = bg.SetSize(Vector2 {
                                X: bar_width,
                                Y: BAR_HEIGHT,
                            });
                            let _ = bg.SetOffset(Vector3 {
                                X: bar_x,
                                Y: bar_y,
                                Z: 0.0,
                            });
                            if let Ok(clip) = bg.Clip() {
                                let geo_clip: windows::UI::Composition::CompositionGeometricClip =
                                    clip.cast().unwrap();
                                if let Ok(geo) = geo_clip.Geometry() {
                                    let rounded: windows::UI::Composition::CompositionRoundedRectangleGeometry = geo.cast().unwrap();
                                    let _ = rounded.SetSize(Vector2 {
                                        X: bar_width,
                                        Y: BAR_HEIGHT,
                                    });
                                }
                            }
                        }
                    }
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => LRESULT(0),
            WM_DESTROY => {
                // Restore Windows taskbar
                if let Some(mutex) = STATE.get() {
                    if let Ok(state) = mutex.lock() {
                        if let Some(tb) = state.taskbar_hwnd {
                            ShowWindow(tb, SW_SHOW);
                        }
                    }
                }
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(window, message, wparam, lparam),
        }
    }
}
