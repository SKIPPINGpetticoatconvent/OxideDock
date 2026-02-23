use crate::config::Config;
use image::RgbaImage;
use std::sync::{Mutex, OnceLock};

use windows::Foundation::Numerics::{Vector2, Vector3};
use windows::Foundation::TimeSpan;
use windows::UI::Composition::{
    Compositor, ContainerVisual, Desktop::DesktopWindowTarget, SpriteVisual,
    Vector3KeyFrameAnimation,
};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_PROPERTIES, ID2D1DeviceContext,
};
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
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
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowW,
    GetMessageW, GetSystemMetrics, IDC_ARROW, LoadCursorW, MSG, PostQuitMessage, RegisterClassExW,
    SM_CXSCREEN, SM_CYSCREEN, SW_HIDE, SW_SHOW, ShowWindow, TranslateMessage, WM_DESTROY,
    WM_LBUTTONUP, WM_MOUSEMOVE, WNDCLASSEXW, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};
use windows::core::{ComInterface, HSTRING, Result, w};

// ═══════════════════════════════════════════════════════════════
// macOS Dock Light Mode - Visual Constants
// ═══════════════════════════════════════════════════════════════
const ICON_SIZE: f32 = 48.0;
const ICON_SPACING: f32 = 10.0;
const ICON_SLOT: f32 = ICON_SIZE + ICON_SPACING;
const BAR_PADDING_H: f32 = 12.0;
const BAR_PADDING_V: f32 = 6.0;
const BAR_HEIGHT: f32 = ICON_SIZE + BAR_PADDING_V * 2.0;
const BAR_CORNER_RADIUS: f32 = 16.0;
const BAR_BOTTOM_MARGIN: f32 = 8.0;
const MAX_SCALE: f32 = 1.8;
const SIGMA: f32 = 90.0;
const ANIM_DURATION: i64 = 800000;

// macOS Light Mode Colors
const BG_ALPHA: u8 = 180;
const BG_R: u8 = 240;
const BG_G: u8 = 240;
const BG_B: u8 = 240;

pub struct AppState {
    pub icon_images: Vec<Option<RgbaImage>>,
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

/// Render an RgbaImage onto a Composition drawing surface via D2D.
/// Converts RGBA → premultiplied BGRA, then draws via D2D bitmap.
fn create_icon_brush(
    compositor: &Compositor,
    comp_graphics_dev: &windows::UI::Composition::CompositionGraphicsDevice,
    img: &RgbaImage,
    idx: usize,
) -> Result<windows::UI::Composition::CompositionSurfaceBrush> {
    unsafe {
        let (w, h) = (img.width(), img.height());
        println!(
            "    [brush] Creating {}x{} surface for icon {}...",
            w, h, idx
        );

        let surface = comp_graphics_dev.CreateDrawingSurface(
            windows::Foundation::Size {
                Width: w as f32,
                Height: h as f32,
            },
            windows::Graphics::DirectX::DirectXPixelFormat::B8G8R8A8UIntNormalized,
            windows::Graphics::DirectX::DirectXAlphaMode::Premultiplied,
        )?;

        let interop: ICompositionDrawingSurfaceInterop = surface.cast()?;
        let mut offset = windows::Win32::Foundation::POINT::default();
        let update_rect = windows::Win32::Foundation::RECT {
            left: 0,
            top: 0,
            right: w as i32,
            bottom: h as i32,
        };

        let d2d_ctx: ID2D1DeviceContext = interop.BeginDraw(Some(&update_rect), &mut offset)?;
        println!(
            "    [brush] BeginDraw OK, offset=({},{})",
            offset.x, offset.y
        );

        // RGBA → premultiplied BGRA
        let mut bgra: Vec<u8> = Vec::with_capacity((w * h * 4) as usize);
        for pixel in img.pixels() {
            let [r, g, b, a] = pixel.0;
            let af = a as f32 / 255.0;
            bgra.push((b as f32 * af) as u8);
            bgra.push((g as f32 * af) as u8);
            bgra.push((r as f32 * af) as u8);
            bgra.push(a);
        }

        let props = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
        };
        let bitmap = d2d_ctx.CreateBitmap(
            D2D_SIZE_U {
                width: w,
                height: h,
            },
            Some(bgra.as_ptr() as *const _),
            w * 4,
            &props,
        )?;

        // Draw directly — BeginDraw from surface interop already started drawing
        d2d_ctx.Clear(None);
        let dest = D2D_RECT_F {
            left: offset.x as f32,
            top: offset.y as f32,
            right: (offset.x as u32 + w) as f32,
            bottom: (offset.y as u32 + h) as f32,
        };
        d2d_ctx.DrawBitmap(
            &bitmap,
            Some(&dest),
            1.0,
            D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
            None,
        );
        // Only call surface interop's EndDraw, NOT d2d_ctx.EndDraw()
        interop.EndDraw()?;

        let brush = compositor.CreateSurfaceBrushWithSurface(&surface)?;
        println!("    [brush] Icon {} surface created OK", idx);
        Ok(brush)
    }
}

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

        let screen_width = GetSystemMetrics(SM_CXSCREEN);
        let screen_height = GetSystemMetrics(SM_CYSCREEN);
        let win_width = screen_width;
        let win_height = 200;
        let win_x = 0;
        let win_y = screen_height - win_height;

        // ═══ Extract icons ═══
        println!("Extracting icons...");
        let mut icon_images: Vec<Option<RgbaImage>> = Vec::new();
        for cat in &config.categories {
            for sc in &cat.shortcuts {
                icon_images.push(crate::icon_extractor::extract_icon(&sc.path));
            }
        }
        let valid = icon_images.iter().filter(|i| i.is_some()).count();
        println!("Icons: {} total, {} valid", icon_images.len(), valid);

        let n_icons = icon_images.len();
        let state_obj = AppState {
            icon_images,
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

        // ═══ Window ═══
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

        // ═══ Composition ═══
        let interop: ICompositorDesktopInterop = compositor.cast()?;
        let target = interop.CreateDesktopWindowTarget(hwnd, true)?;
        let root_visual = compositor.CreateContainerVisual()?;
        target.SetRoot(&root_visual)?;

        // Background bar
        let bg_bar_visual = compositor.CreateSpriteVisual()?;
        let bg_brush = compositor.CreateColorBrushWithColor(windows::UI::Color {
            A: BG_ALPHA,
            R: BG_R,
            G: BG_G,
            B: BG_B,
        })?;
        bg_bar_visual.SetBrush(&bg_brush)?;

        // Rounded corners
        let rounded_rect_opt = compositor.CreateRoundedRectangleGeometry().ok();
        if let Some(ref rr) = rounded_rect_opt {
            let _ = rr.SetCornerRadius(Vector2 {
                X: BAR_CORNER_RADIUS,
                Y: BAR_CORNER_RADIUS,
            });
            if let Ok(clip) = compositor.CreateGeometricClip() {
                let _ = clip.SetGeometry(rr);
                let _ = bg_bar_visual.SetClip(&clip);
            }
        }
        root_visual.Children()?.InsertAtBottom(&bg_bar_visual)?;

        // ═══ D3D11 for Composition Graphics ═══
        // Try hardware first, fall back to WARP (software)
        let mut d3d_device: Option<ID3D11Device> = None;
        let hw_result = D3D11CreateDevice(
            None,
            windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE,
            None,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut d3d_device),
            None,
            None,
        );
        if hw_result.is_err() || d3d_device.is_none() {
            println!("D3D11 Hardware failed, trying WARP...");
            D3D11CreateDevice(
                None,
                windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_WARP,
                None,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut d3d_device),
                None,
                None,
            )?;
        }
        let d3d_device = d3d_device.unwrap();
        let dxgi_device: IDXGIDevice = d3d_device.cast()?;
        let interop2: ICompositorInterop = compositor.cast()?;
        let comp_graphics_dev = interop2.CreateGraphicsDevice(&dxgi_device)?;
        println!("D3D11 + CompositionGraphicsDevice created OK");

        // ═══ Layout ═══
        let total_icons_width = n_icons as f32 * ICON_SLOT - ICON_SPACING;
        let bar_total_width = total_icons_width + BAR_PADDING_H * 2.0;
        let bar_x = (win_width as f32 - bar_total_width) / 2.0;
        let bar_y = win_height as f32 - BAR_HEIGHT - BAR_BOTTOM_MARGIN;
        let icons_start_x = bar_x + BAR_PADDING_H;
        let icon_y = bar_y + BAR_PADDING_V;

        let _ = bg_bar_visual.SetSize(Vector2 {
            X: bar_total_width,
            Y: BAR_HEIGHT,
        });
        let _ = bg_bar_visual.SetOffset(Vector3 {
            X: bar_x,
            Y: bar_y,
            Z: 0.0,
        });
        if let Some(ref rr) = rounded_rect_opt {
            let _ = rr.SetSize(Vector2 {
                X: bar_total_width,
                Y: BAR_HEIGHT,
            });
        }

        // ═══ Icon Visuals ═══
        let mut current_x = icons_start_x;
        if let Some(mutex) = STATE.get() {
            if let Ok(mut state) = mutex.lock() {
                state.bg_bar_visual = Some(bg_bar_visual.clone());
                let images: Vec<Option<RgbaImage>> = state.icon_images.drain(..).collect();

                for (idx, img_opt) in images.into_iter().enumerate() {
                    if let Some(ref img) = img_opt {
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

                        // Try to render icon, fall back to colored placeholder
                        match create_icon_brush(&compositor, &comp_graphics_dev, img, idx) {
                            Ok(brush) => {
                                icon_visual.SetBrush(&brush)?;
                                println!("  Icon {} rendered OK", idx);
                            }
                            Err(e) => {
                                println!("  Icon {} render failed: {:?}, placeholder", idx, e);
                                let colors = [
                                    windows::UI::Color {
                                        A: 255,
                                        R: 66,
                                        G: 133,
                                        B: 244,
                                    },
                                    windows::UI::Color {
                                        A: 255,
                                        R: 234,
                                        G: 67,
                                        B: 53,
                                    },
                                    windows::UI::Color {
                                        A: 255,
                                        R: 251,
                                        G: 188,
                                        B: 4,
                                    },
                                    windows::UI::Color {
                                        A: 255,
                                        R: 52,
                                        G: 168,
                                        B: 83,
                                    },
                                ];
                                let brush =
                                    compositor.CreateColorBrushWithColor(colors[idx % 4])?;
                                icon_visual.SetBrush(&brush)?;
                            }
                        }
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

        // ═══ Hide Taskbar ═══
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

        // ═══ Message Loop ═══
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).into() {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════
// Layout calculation
// ═══════════════════════════════════════════════════════════════

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
            scales[i] = 1.0 + (MAX_SCALE - 1.0) * (-(dist * dist) / (2.0 * SIGMA * SIGMA)).exp();
            icon_widths[i] = scales[i] * ICON_SIZE;
        }
    }
    let total_width: f32 = icon_widths.iter().sum::<f32>() + (n as f32 - 1.0) * ICON_SPACING;
    let bar_width = total_width + BAR_PADDING_H * 2.0;
    let bar_x = (win_width as f32 - bar_width) / 2.0;
    let bar_y = win_height as f32 - BAR_HEIGHT - BAR_BOTTOM_MARGIN;
    let mut positions = Vec::with_capacity(n);
    let mut cx = bar_x + BAR_PADDING_H;
    for i in 0..n {
        let sy = scales[i] * ICON_SIZE;
        positions.push(Vector3 {
            X: cx,
            Y: bar_y + BAR_HEIGHT - BAR_PADDING_V - sy,
            Z: 0.0,
        });
        cx += icon_widths[i] + ICON_SPACING;
    }
    (scales, positions, bar_width, bar_x, bar_y)
}

extern "system" fn wndproc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match message {
            WM_MOUSEMOVE => {
                let mouse_x = (lparam.0 & 0xFFFF) as i32 as f32;
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
                            let v = &state.icon_visuals[i];
                            let sa = &state.scale_animations[i];
                            let oa = &state.offset_animations[i];
                            let _ = sa.InsertKeyFrame(
                                1.0,
                                Vector3 {
                                    X: scales[i],
                                    Y: scales[i],
                                    Z: 1.0,
                                },
                            );
                            let _ = v.StartAnimation(&HSTRING::from("Scale"), sa);
                            let _ = oa.InsertKeyFrame(1.0, positions[i]);
                            let _ = v.StartAnimation(&HSTRING::from("Offset"), oa);
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
                                if let Ok(gc) = clip
                                    .cast::<windows::UI::Composition::CompositionGeometricClip>(
                                ) {
                                    if let Ok(geo) = gc.Geometry() {
                                        if let Ok(rr) = geo.cast::<windows::UI::Composition::CompositionRoundedRectangleGeometry>() {
                                            let _ = rr.SetSize(Vector2 { X: bar_width, Y: BAR_HEIGHT });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                LRESULT(0)
            }
            0x02A3 => {
                // WM_MOUSELEAVE
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
                            let v = &state.icon_visuals[i];
                            let sa = &state.scale_animations[i];
                            let oa = &state.offset_animations[i];
                            let _ = sa.InsertKeyFrame(
                                1.0,
                                Vector3 {
                                    X: 1.0,
                                    Y: 1.0,
                                    Z: 1.0,
                                },
                            );
                            let _ = v.StartAnimation(&HSTRING::from("Scale"), sa);
                            let _ = oa.InsertKeyFrame(1.0, positions[i]);
                            let _ = v.StartAnimation(&HSTRING::from("Offset"), oa);
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
                                if let Ok(gc) = clip
                                    .cast::<windows::UI::Composition::CompositionGeometricClip>(
                                ) {
                                    if let Ok(geo) = gc.Geometry() {
                                        if let Ok(rr) = geo.cast::<windows::UI::Composition::CompositionRoundedRectangleGeometry>() {
                                            let _ = rr.SetSize(Vector2 { X: bar_width, Y: BAR_HEIGHT });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => LRESULT(0),
            WM_DESTROY => {
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
