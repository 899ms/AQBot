//! Resolve application identity from user-picked paths for the selection-toolbar
//! app filter.
//!
//! Identity of each entry is aligned with `SelectionObservation.source_app`:
//! - macOS: bundle identifier
//! - Windows: lower-case executable basename (e.g. `notepad.exe`)
//! - Linux: desktop file id / StartupWMClass / name

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const ICON_SIZE: u32 = 64;
/// After resize a 64×64 PNG is tiny; keep a generous cap for safety.
const MAX_ICON_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct InstalledApp {
    pub id: String,
    pub name: String,
    pub icon_data_url: Option<String>,
}

/// Resolve one or more user-selected paths into app filter entries.
/// Invalid paths are skipped; order follows first occurrence of each id.
pub fn resolve_app_paths(paths: &[String]) -> Result<Vec<InstalledApp>, String> {
    let mut seen = BTreeMap::new();
    let mut order = Vec::new();
    for raw in paths {
        let path = PathBuf::from(raw.trim());
        if path.as_os_str().is_empty() {
            continue;
        }
        let Some(app) = resolve_path(&path) else {
            tracing::debug!(path = %path.display(), "skipped unrecognised app path");
            continue;
        };
        if !seen.contains_key(&app.id) {
            order.push(app.id.clone());
            seen.insert(app.id.clone(), app);
        }
    }
    Ok(order
        .into_iter()
        .filter_map(|id| seen.remove(&id))
        .collect())
}

/// Best-effort icon lookup for already-saved filter ids (no full disk scan).
#[cfg(not(target_os = "macos"))]
pub fn resolve_app_icons(ids: &[String]) -> Result<BTreeMap<String, String>, String> {
    let mut map = BTreeMap::new();
    for id in ids {
        if let Some(icon) = icon_for_id(id) {
            map.insert(id.clone(), icon);
        }
    }
    Ok(map)
}

fn encode_png_data_url(png: &[u8]) -> Option<String> {
    if png.is_empty() || png.len() > MAX_ICON_BYTES {
        return None;
    }
    Some(format!("data:image/png;base64,{}", BASE64.encode(png)))
}

fn png_data_url_from_image_bytes(data: &[u8]) -> Option<String> {
    let dyn_img = image::load_from_memory(data).ok()?;
    let rgba = dyn_img.to_rgba8();
    let png = resize_rgba_to_png(rgba.as_raw(), rgba.width(), rgba.height())?;
    encode_png_data_url(&png)
}

fn resize_rgba_to_png(rgba: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    use image::{imageops::FilterType, ImageBuffer, ImageFormat, RgbaImage};
    if width == 0 || height == 0 {
        return None;
    }
    let image: RgbaImage = ImageBuffer::from_raw(width, height, rgba.to_vec())?;
    let resized = if width == ICON_SIZE && height == ICON_SIZE {
        image
    } else {
        image::imageops::resize(&image, ICON_SIZE, ICON_SIZE, FilterType::Lanczos3)
    };
    let mut buf = std::io::Cursor::new(Vec::new());
    resized.write_to(&mut buf, ImageFormat::Png).ok()?;
    let bytes = buf.into_inner();
    (bytes.len() <= MAX_ICON_BYTES).then_some(bytes)
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn load_image_file_as_png_data_url(path: &Path) -> Option<String> {
    let data = std::fs::read(path).ok()?;
    png_data_url_from_image_bytes(&data)
}

#[cfg(target_os = "macos")]
fn resolve_path(path: &Path) -> Option<InstalledApp> {
    macos::from_path(path)
}

#[cfg(target_os = "windows")]
fn resolve_path(path: &Path) -> Option<InstalledApp> {
    windows::from_path(path)
}

#[cfg(target_os = "linux")]
fn resolve_path(path: &Path) -> Option<InstalledApp> {
    linux::from_path(path)
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn resolve_path(_path: &Path) -> Option<InstalledApp> {
    None
}

#[cfg(target_os = "macos")]
pub fn resolve_app_icon_sources(ids: &[String]) -> BTreeMap<String, Vec<u8>> {
    ids.iter()
        .filter_map(|id| macos::icon_source_for_bundle_id(id).map(|source| (id.clone(), source)))
        .collect()
}

#[cfg(target_os = "macos")]
pub fn encode_app_icon_sources(sources: BTreeMap<String, Vec<u8>>) -> BTreeMap<String, String> {
    sources
        .into_iter()
        .filter_map(|(id, source)| encode_png_data_url(&source).map(|icon| (id, icon)))
        .collect()
}

#[cfg(not(target_os = "macos"))]
fn icon_for_id(_id: &str) -> Option<String> {
    None
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSBitmapImageFileType, NSBitmapImageRep, NSCompositingOperation, NSDeviceRGBColorSpace,
        NSGraphicsContext, NSWorkspace,
    };
    use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize, NSString};
    use std::ffi::OsStr;

    pub fn from_path(path: &Path) -> Option<InstalledApp> {
        let bundle = normalize_bundle_path(path)?;
        app_from_bundle(&bundle)
    }

    pub fn icon_source_for_bundle_id(bundle_id: &str) -> Option<Vec<u8>> {
        let id = bundle_id.trim();
        if id.is_empty() {
            return None;
        }
        let workspace = NSWorkspace::sharedWorkspace();
        let ns_id = NSString::from_str(id);
        let url = workspace.URLForApplicationWithBundleIdentifier(&ns_id)?;
        let path = url.path()?.to_string();
        icon_source_for_bundle(Path::new(&path))
    }

    fn normalize_bundle_path(path: &Path) -> Option<PathBuf> {
        if path.extension().and_then(OsStr::to_str) == Some("app") {
            return path.exists().then(|| path.to_path_buf());
        }
        // User may pick an inner file; walk up for *.app
        for ancestor in path.ancestors() {
            if ancestor.extension().and_then(OsStr::to_str) == Some("app") && ancestor.exists() {
                return Some(ancestor.to_path_buf());
            }
        }
        None
    }

    fn app_from_bundle(bundle: &Path) -> Option<InstalledApp> {
        let info_path = bundle.join("Contents/Info.plist");
        let info = plist::Value::from_file(&info_path).ok()?;
        let dict = info.as_dictionary()?;
        let id = dict
            .get("CFBundleIdentifier")?
            .as_string()?
            .trim()
            .to_string();
        if id.is_empty() {
            return None;
        }
        let name = dict
            .get("CFBundleDisplayName")
            .or_else(|| dict.get("CFBundleName"))
            .and_then(|value| value.as_string())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                bundle
                    .file_stem()
                    .and_then(OsStr::to_str)
                    .map(str::to_string)
            })?;
        Some(InstalledApp {
            id,
            name,
            // Icon extraction is intentionally deferred so resolving the picked
            // app metadata never blocks the UI on AppKit image work.
            icon_data_url: None,
        })
    }

    fn icon_source_for_bundle(bundle: &Path) -> Option<Vec<u8>> {
        let path = bundle.to_str()?;
        let workspace = NSWorkspace::sharedWorkspace();
        let ns_path = NSString::from_str(path);
        let image = workspace.iconForFile(&ns_path);
        let mtm = MainThreadMarker::new()?;
        let bitmap = unsafe {
            NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                mtm.alloc(),
                std::ptr::null_mut(),
                ICON_SIZE as isize,
                ICON_SIZE as isize,
                8,
                4,
                true,
                false,
                NSDeviceRGBColorSpace,
                0,
                0,
            )
        }?;
        let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&bitmap)?;
        let rect = NSRect::new(
            NSPoint::ZERO,
            NSSize::new(ICON_SIZE as f64, ICON_SIZE as f64),
        );
        NSGraphicsContext::saveGraphicsState_class();
        NSGraphicsContext::setCurrentContext(Some(&context));
        image.drawInRect_fromRect_operation_fraction(
            rect,
            NSRect::ZERO,
            NSCompositingOperation::Copy,
            1.0,
        );
        context.flushGraphics();
        NSGraphicsContext::restoreGraphicsState_class();

        let properties = NSDictionary::new();
        let png = unsafe {
            bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &properties)
        }?;
        let bytes = png.to_vec();
        (!bytes.is_empty()).then_some(bytes)
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use std::ffi::OsStr;

    pub fn from_path(path: &Path) -> Option<InstalledApp> {
        if !path.is_file() {
            return None;
        }
        let ext = path
            .extension()
            .and_then(OsStr::to_str)
            .map(|value| value.to_ascii_lowercase());
        match ext.as_deref() {
            Some("exe") => app_from_exe(path),
            Some("lnk") => app_from_shortcut(path),
            _ => None,
        }
    }

    fn app_from_exe(path: &Path) -> Option<InstalledApp> {
        let id = path
            .file_name()
            .and_then(OsStr::to_str)?
            .to_ascii_lowercase();
        if id.is_empty() || id == "uninstall.exe" || id.starts_with("unins") {
            return None;
        }
        let name = path
            .file_stem()
            .and_then(OsStr::to_str)
            .unwrap_or(&id)
            .to_string();
        Some(InstalledApp {
            id,
            name,
            icon_data_url: None,
        })
    }

    fn app_from_shortcut(path: &Path) -> Option<InstalledApp> {
        use ::windows::core::{Interface, PCWSTR};
        use ::windows::Win32::Foundation::MAX_PATH;
        use ::windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
            COINIT_APARTMENTTHREADED,
        };
        use ::windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
        use std::os::windows::ffi::OsStrExt;

        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let shell: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
            let persist: IPersistFile = shell.cast().ok()?;
            let wide: Vec<u16> = OsStr::new(path)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            persist
                .Load(PCWSTR(wide.as_ptr()), Default::default())
                .ok()?;
            let mut buffer = vec![0u16; MAX_PATH as usize];
            shell.GetPath(&mut buffer, std::ptr::null_mut(), 0).ok()?;
            let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
            if len == 0 {
                CoUninitialize();
                return None;
            }
            let target = PathBuf::from(String::from_utf16_lossy(&buffer[..len]));
            CoUninitialize();
            if target
                .extension()
                .and_then(OsStr::to_str)
                .map(|ext| ext.eq_ignore_ascii_case("exe"))
                != Some(true)
            {
                return None;
            }
            let id = target
                .file_name()
                .and_then(OsStr::to_str)?
                .to_ascii_lowercase();
            let name = path
                .file_stem()
                .and_then(OsStr::to_str)
                .map(str::to_string)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| {
                    target
                        .file_stem()
                        .and_then(OsStr::to_str)
                        .unwrap_or(&id)
                        .to_string()
                });
            Some(InstalledApp {
                id,
                name,
                icon_data_url: None,
            })
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::ffi::OsStr;

    pub fn from_path(path: &Path) -> Option<InstalledApp> {
        if path.extension().and_then(OsStr::to_str) == Some("desktop") {
            return app_from_desktop(path);
        }
        if path.is_file() {
            // Allow picking a binary: use basename as id/name.
            let id = path.file_name().and_then(OsStr::to_str)?.to_string();
            if id.is_empty() {
                return None;
            }
            return Some(InstalledApp {
                id: id.clone(),
                name: id,
                icon_data_url: None,
            });
        }
        None
    }

    fn app_from_desktop(path: &Path) -> Option<InstalledApp> {
        let content = std::fs::read_to_string(path).ok()?;
        let mut in_desktop_entry = false;
        let mut name = None;
        let mut icon = None;
        let mut no_display = false;
        let mut hidden = false;
        let mut startup_wm_class = None;
        let mut type_app = false;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                in_desktop_entry = line == "[Desktop Entry]";
                continue;
            }
            if !in_desktop_entry {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                match key {
                    "Type" => type_app = value == "Application",
                    "Name" => name = Some(value.to_string()),
                    "Icon" => icon = Some(value.to_string()),
                    "NoDisplay" => no_display = value.eq_ignore_ascii_case("true"),
                    "Hidden" => hidden = value.eq_ignore_ascii_case("true"),
                    "StartupWMClass" => startup_wm_class = Some(value.to_string()),
                    _ => {}
                }
            }
        }

        if (!type_app && name.is_none()) || no_display || hidden {
            // Still accept if Name exists even when Type is missing (some entries).
            if name.is_none() {
                return None;
            }
        }
        let name = name.filter(|value| !value.trim().is_empty())?;
        let file_id = path
            .file_name()
            .and_then(OsStr::to_str)
            .map(|value| value.trim_end_matches(".desktop").to_string())?;
        let id = startup_wm_class
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(file_id);
        let icon_data_url = icon.as_deref().and_then(resolve_icon_data_url);
        Some(InstalledApp {
            id,
            name: name.trim().to_string(),
            icon_data_url,
        })
    }

    fn resolve_icon_data_url(icon: &str) -> Option<String> {
        let path = Path::new(icon);
        if path.is_absolute() && path.is_file() {
            return load_image_file_as_png_data_url(path);
        }
        let name = icon.trim();
        if name.is_empty() || name.contains('/') {
            return None;
        }
        let sizes = ["32x32", "48x48", "64x64", "24x24", "16x16", "scalable"];
        let mut roots = vec![
            PathBuf::from("/usr/share/icons/hicolor"),
            PathBuf::from("/usr/share/pixmaps"),
        ];
        if let Some(home) = dirs::home_dir() {
            roots.push(home.join(".local/share/icons/hicolor"));
        }
        for root in roots {
            if root.ends_with("pixmaps") {
                for ext in ["png", "svg", "xpm"] {
                    let candidate = root.join(format!("{name}.{ext}"));
                    if let Some(url) = load_image_file_as_png_data_url(&candidate) {
                        return Some(url);
                    }
                }
                continue;
            }
            for size in sizes {
                for ext in ["png", "svg"] {
                    let candidate = root.join(size).join("apps").join(format!("{name}.{ext}"));
                    if let Some(url) = load_image_file_as_png_data_url(&candidate) {
                        return Some(url);
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_app_paths_skips_empty_and_dedupes() {
        let apps = resolve_app_paths(&["  ".into(), "".into()]).expect("ok");
        assert!(apps.is_empty());
    }

    #[test]
    fn encode_png_data_url_rejects_empty() {
        assert!(encode_png_data_url(&[]).is_none());
    }

    #[test]
    fn app_icons_are_rendered_at_retina_resolution() {
        let rgba = vec![255; 128 * 128 * 4];
        let png = resize_rgba_to_png(&rgba, 128, 128).expect("png");
        let image = image::load_from_memory(&png).expect("decoded png");
        assert_eq!((image.width(), image.height()), (64, 64));
    }
}
