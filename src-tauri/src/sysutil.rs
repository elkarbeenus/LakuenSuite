//! Systemnahe Helfer: sauberes Starten aus dem AppImage heraus, Icons, Startmenü-Einträge.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use crate::store;
use crate::text;

/// Variablen, die die AppImage-Laufzeit (linuxdeploy/GTK) setzt und die gestartete
/// Programme nicht erben dürfen, sonst laden sie falsche Bibliotheken.
const APPIMAGE_VARS: &[&str] = &[
    "APPDIR", "APPIMAGE", "ARGV0", "OWD", "APPIMAGE_UUID",
    "GTK_DATA_PREFIX", "GTK_EXE_PREFIX", "GTK_PATH", "GTK_IM_MODULE_FILE", "GTK_THEME",
    "GDK_PIXBUF_MODULE_FILE", "GDK_PIXBUF_MODULEDIR", "GDK_BACKEND",
    "GIO_EXTRA_MODULES", "GIO_MODULE_DIR", "GSETTINGS_SCHEMA_DIR",
    "GST_PLUGIN_SYSTEM_PATH", "GST_PLUGIN_SYSTEM_PATH_1_0", "GST_PLUGIN_PATH", "GST_PLUGIN_PATH_1_0",
    "GST_PLUGIN_SCANNER", "GST_PLUGIN_SCANNER_1_0", "GST_REGISTRY", "GST_REGISTRY_1_0",
    "LD_PRELOAD", "PYTHONHOME", "PYTHONPATH", "PERLLIB", "QT_PLUGIN_PATH",
    "WEBKIT_EXEC_PATH", "WEBKIT_INJECTED_BUNDLE_PATH",
];
const PATH_LISTS: &[&str] = &["LD_LIBRARY_PATH", "PATH", "XDG_DATA_DIRS", "XDG_CONFIG_DIRS"];

pub fn clean_env(cmd: &mut Command) {
    if std::env::var("LAKUENSUITE_SET_DMABUF").is_ok() {
        cmd.env_remove("WEBKIT_DISABLE_DMABUF_RENDERER");
    }
    cmd.env_remove("LAKUENSUITE_SET_DMABUF");
    let appdir = match std::env::var("APPDIR") {
        Ok(a) if !a.is_empty() => a,
        _ => return, // nicht im AppImage: nichts zu bereinigen
    };
    for k in APPIMAGE_VARS {
        cmd.env_remove(k);
    }
    for k in PATH_LISTS {
        if let Ok(v) = std::env::var(k) {
            let kept: Vec<&str> = v.split(':').filter(|p| !p.is_empty() && !p.starts_with(&appdir)).collect();
            if kept.is_empty() {
                if *k == "PATH" {
                    cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin");
                } else {
                    cmd.env_remove(k);
                }
            } else {
                cmd.env(k, kept.join(":"));
            }
        }
    }
}

/// Programm losgelöst starten (läuft weiter, wenn LakuenSuite geschlossen wird).
pub fn spawn_detached(program: &Path, args: &[&str]) -> Result<Child, String> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(dir) = program.parent() {
        cmd.current_dir(dir);
    }
    clean_env(&mut cmd);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn().map_err(|e| format!("Start fehlgeschlagen: {e}"))
}

pub fn open_external(target: &str) -> Result<(), String> {
    let mut cmd = Command::new("xdg-open");
    cmd.arg(target).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    clean_env(&mut cmd);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn().map(|_| ()).map_err(|e| format!("Konnte nicht öffnen: {e}"))
}

#[cfg(unix)]
pub fn make_executable(p: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())
}
#[cfg(not(unix))]
pub fn make_executable(_p: &Path) -> Result<(), String> {
    Ok(())
}

/// Befehl mit Zeitlimit ausführen; true, wenn er rechtzeitig fertig wurde.
fn run_with_timeout(mut cmd: Command, secs: u64) -> bool {
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => {
                if start.elapsed().as_secs() >= secs {
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(_) => return false,
        }
    }
}

fn icon_ext(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG") {
        Some("png")
    } else {
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).to_lowercase();
        if head.contains("<svg") || head.contains("<?xml") {
            Some("svg")
        } else {
            None
        }
    }
}

/// Icon aus dem AppImage holen (.DirIcon, Symlinks werden verfolgt). Best effort.
pub fn extract_icon(appimage: &Path, repo: &str) -> Option<PathBuf> {
    let work = std::env::temp_dir().join(format!("lakuensuite-icon-{}-{}", repo, std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).ok()?;
    let mut wanted = String::from(".DirIcon");
    let mut result = None;
    for _ in 0..4 {
        let mut cmd = Command::new(appimage);
        cmd.arg("--appimage-extract")
            .arg(&wanted)
            .current_dir(&work)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        clean_env(&mut cmd);
        if !run_with_timeout(cmd, 15) {
            break;
        }
        let p = work.join("squashfs-root").join(&wanted);
        match std::fs::symlink_metadata(&p) {
            Ok(m) if m.file_type().is_symlink() => {
                let target = match std::fs::read_link(&p) {
                    Ok(t) => t,
                    Err(_) => break,
                };
                let t = target.to_string_lossy().trim_start_matches("./").trim_start_matches('/').to_string();
                // relativ zum Ordner des Links
                let base = Path::new(&wanted).parent().map(|b| b.to_string_lossy().to_string()).unwrap_or_default();
                wanted = if base.is_empty() || target.is_absolute() { t } else { format!("{base}/{t}") };
                continue;
            }
            Ok(_) => {
                if let Ok(bytes) = std::fs::read(&p) {
                    if let Some(ext) = icon_ext(&bytes) {
                        let dir = store::icons_dir();
                        let _ = std::fs::create_dir_all(&dir);
                        let _ = std::fs::remove_file(dir.join(format!("{repo}.png")));
                        let _ = std::fs::remove_file(dir.join(format!("{repo}.svg")));
                        let out = dir.join(format!("{repo}.{ext}"));
                        if std::fs::write(&out, &bytes).is_ok() {
                            result = Some(out);
                        }
                    }
                }
                break;
            }
            Err(_) => break,
        }
    }
    let _ = std::fs::remove_dir_all(&work);
    result
}

/// Eigenes Icon aus dem laufenden AppImage kopieren.
pub fn own_icon() -> Option<PathBuf> {
    let appdir = std::env::var("APPDIR").ok()?;
    let src = Path::new(&appdir).join(".DirIcon");
    let bytes = std::fs::read(&src).ok()?; // folgt Symlinks
    let ext = icon_ext(&bytes)?;
    let dir = store::icons_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let out = dir.join(format!("lakuensuite.{ext}"));
    std::fs::write(&out, bytes).ok()?;
    Some(out)
}

pub fn desktop_file(repo: &str) -> PathBuf {
    store::applications_dir().join(format!("lakuensuite-{}.desktop", repo.to_lowercase()))
}

pub fn write_desktop_entry(id: &str, name: &str, comment: &str, exec: &Path, icon: Option<&Path>) -> Result<PathBuf, String> {
    let dir = store::applications_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = desktop_file(id);
    let icon = icon
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "application-x-executable".into());
    let content = format!(
        "[Desktop Entry]\nType=Application\nName={}\nComment={}\nExec={}\nIcon={}\nTerminal=false\nCategories=Utility;\nX-LakuenSuite-Repo={}\n",
        text::desktop_value(name),
        text::desktop_value(comment),
        text::desktop_exec_quote(&exec.to_string_lossy()),
        text::desktop_value(&icon),
        text::desktop_value(id),
    );
    std::fs::write(&file, content).map_err(|e| e.to_string())?;
    let _ = make_executable(&file);
    Ok(file)
}

pub fn remove_desktop_entry(id: &str) {
    let _ = std::fs::remove_file(desktop_file(id));
}

pub fn file_to_data_url(p: &Path) -> Option<String> {
    use base64::Engine;
    let bytes = std::fs::read(p).ok()?;
    if bytes.len() > 2_000_000 {
        return None;
    }
    let mime = match icon_ext(&bytes)? {
        "png" => "image/png",
        _ => "image/svg+xml",
    };
    Some(format!(
        "data:{};base64,{}",
        mime,
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}
