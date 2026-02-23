use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[cfg(target_os = "windows")]
use windows::{
    Win32::System::Com::{
        CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, IPersistFile,
        STGM_READ,
    },
    Win32::UI::Shell::{IShellLinkW, ShellLink},
    core::{ComInterface, PCWSTR},
};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Shortcut {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Category {
    pub name: String,
    pub shortcuts: Vec<Shortcut>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub categories: Vec<Category>,
}

pub fn load_config<P: AsRef<Path>>(path: P) -> Result<Config, Box<dyn std::error::Error>> {
    let mut config = if path.as_ref().exists() {
        let content = fs::read_to_string(path)?;
        serde_json::from_str(&content)?
    } else {
        Config { categories: vec![] }
    };

    // Auto-discover pinned items and add them as a "Pinned" category if not empty
    let pinned = discover_pinned_items();
    if !pinned.is_empty() {
        config.categories.push(Category {
            name: "Pinned".to_string(),
            shortcuts: pinned,
        });
    }

    Ok(config)
}

fn discover_pinned_items() -> Vec<Shortcut> {
    let mut shortcuts = Vec::new();

    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            let pinned_path = Path::new(&appdata)
                .join(r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar");
            if let Ok(entries) = fs::read_dir(pinned_path) {
                // Initialize COM for shortcut resolution
                unsafe {
                    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                }

                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|s| s.to_str()) == Some("lnk") {
                        if let Some(target) = resolve_shortcut(&path) {
                            let name = path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("Unknown")
                                .to_string();
                            shortcuts.push(Shortcut { name, path: target });
                        }
                    }
                }
            }
        }
    }

    shortcuts
}

#[cfg(target_os = "windows")]
fn resolve_shortcut(lnk_path: &Path) -> Option<String> {
    unsafe {
        let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_ALL).ok()?;
        let persist_file: IPersistFile = shell_link.cast().ok()?;

        let path_wide: Vec<u16> = lnk_path
            .to_str()?
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        persist_file
            .Load(PCWSTR(path_wide.as_ptr()), STGM_READ)
            .ok()?;

        let mut buffer = [0u16; 260];
        if shell_link
            .GetPath(&mut buffer, std::ptr::null_mut(), 0)
            .is_ok()
        {
            let target = String::from_utf16_lossy(&buffer);
            let target = target.trim_matches('\0').to_string();
            if !target.is_empty() {
                return Some(target);
            }
        }
    }
    None
}
