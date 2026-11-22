#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod github;
mod store;
mod sysutil;
mod text;
mod version;

use github::{Api, Asset, Release, Repo};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use store::{Installed, Settings, State, OWNER, SELF_REPO};
use tauri::{AppHandle, Emitter};
use version::OwnVersion;

const SELF_KEY: &str = "__self__";

struct Suite {
    api: Api,
    own: OwnVersion,
    settings: Mutex<Settings>,
    state: Mutex<State>,
    cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
    running: Mutex<HashMap<String, Child>>,
    /// Zuletzt geladene Repos mit ihrem neuesten Release.
    known: Mutex<HashMap<String, (Repo, Option<Release>)>>,
    busy: Mutex<Vec<String>>,
}

impl Suite {
    fn token(&self) -> String {
        self.settings.lock().unwrap().github_token.trim().to_string()
    }
    fn save_state(&self) -> Result<(), String> {
        let st = self.state.lock().unwrap().clone();
        store::save(&store::state_file(), &st, false)
    }
}

// ---------- Datenstrukturen für die Oberfläche ----------

#[derive(Serialize, Clone)]
struct AssetInfo {
    id: u64,
    name: String,
    size: u64,
    has_checksum: bool,
    downloads: u64,
}

#[derive(Serialize, Clone)]
struct ReleaseInfo {
    tag: String,
    name: String,
    body: String,
    url: String,
    published_at: Option<String>,
    asset: Option<AssetInfo>,
}

#[derive(Serialize, Clone)]
struct AppEntry {
    repo: String,
    description: String,
    url: String,
    homepage: Option<String>,
    fork: bool,
    archived: bool,
    private: bool,
    stars: u64,
    language: Option<String>,
    topics: Vec<String>,
    pushed_at: Option<String>,
    release: Option<ReleaseInfo>,
    installed: Option<Installed>,
    icon: Option<String>,
    /// no_release | no_asset | install | update | ok
    status: String,
    held: bool,
    running: bool,
    busy: bool,
    has_previous: bool,
}

#[derive(Serialize, Clone)]
struct SelfUpdate {
    tag: String,
    name: String,
    body: String,
    url: String,
    size: u64,
}

#[derive(Serialize)]
struct ListResult {
    apps: Vec<AppEntry>,
    self_update: Option<SelfUpdate>,
    offline: bool,
    rate_limited: bool,
    rate_remaining: i64,
    rate_reset: i64,
    error: Option<String>,
}

#[derive(Serialize)]
struct Meta {
    version: String,
    base_tag: String,
    owner: String,
    self_repo: String,
    is_appimage: bool,
    appimage_path: Option<String>,
    settings: Settings,
    default_install_dir: String,
    arch: String,
}

#[derive(Serialize, Clone)]
struct Progress {
    repo: String,
    phase: String,
    done: u64,
    total: u64,
    speed: u64,
    message: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SuiteFile {
    name: Option<String>,
    short: Option<String>,
    category: Option<String>,
}

#[derive(Serialize)]
struct Details {
    name: Option<String>,
    short: Option<String>,
    category: Option<String>,
    readme: Option<String>,
}

fn emit(app: &AppHandle, repo: &str, phase: &str, done: u64, total: u64, speed: u64, message: &str) {
    let _ = app.emit(
        "progress",
        Progress { repo: repo.into(), phase: phase.into(), done, total, speed, message: message.into() },
    );
}

fn app_paths(install_dir: &str, repo: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let dir = Path::new(install_dir).join(repo);
    let fin = dir.join(format!("{repo}.AppImage"));
    let prev = dir.join(format!("{repo}.prev.AppImage"));
    let part = dir.join(format!(".{repo}.AppImage.part"));
    (dir, fin, prev, part)
}

fn release_info(rel: &Release) -> ReleaseInfo {
    let asset = github::pick_appimage(&rel.assets, None).map(|a| AssetInfo {
        id: a.id,
        name: a.name.clone(),
        size: a.size,
        has_checksum: a.digest.is_some()
            || rel.assets.iter().any(|x| {
                let n = x.name.to_lowercase();
                n.contains("sha256") || n.contains("checksum")
            }),
        downloads: a.download_count,
    });
    ReleaseInfo {
        tag: rel.tag_name.clone(),
        name: rel.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| rel.tag_name.clone()),
        body: rel.body.clone().unwrap_or_default(),
        url: rel.html_url.clone(),
        published_at: rel.published_at.clone(),
        asset,
    }
}

fn build_entry(suite: &Suite, repo: &Repo, rel: &Option<Release>) -> AppEntry {
    let installed = suite.state.lock().unwrap().installed.get(&repo.name).cloned();
    let running = {
        let mut r = suite.running.lock().unwrap();
        match r.get_mut(&repo.name) {
            Some(c) => matches!(c.try_wait(), Ok(None)),
            None => false,
        }
    };
    let busy = suite.busy.lock().unwrap().contains(&repo.name);
    let rinfo = rel.as_ref().map(release_info);
    let mut held = false;
    let status = match &rinfo {
        None => "no_release".to_string(),
        Some(r) => match &r.asset {
            None => {
                if installed.is_some() { "ok".into() } else { "no_asset".into() }
            }
            Some(a) => {
                let inst = installed.as_ref().map(|i| (i.tag.as_str(), i.asset_id));
                match version::app_state(inst, (r.tag.as_str(), a.id)) {
                    version::AppState::NotInstalled => "install".into(),
                    version::AppState::UpToDate => "ok".into(),
                    version::AppState::Update => {
                        held = installed.as_ref().and_then(|i| i.hold_tag.clone()).as_deref() == Some(r.tag.as_str());
                        "update".into()
                    }
                }
            }
        },
    };
    let icon = installed
        .as_ref()
        .and_then(|i| i.icon.clone())
        .and_then(|p| sysutil::file_to_data_url(Path::new(&p)));
    let has_previous = installed
        .as_ref()
        .map(|i| i.prev_tag.is_some() && Path::new(&i.path).with_file_name(format!("{}.prev.AppImage", repo.name)).exists())
        .unwrap_or(false);
    AppEntry {
        repo: repo.name.clone(),
        description: repo.description.clone().unwrap_or_default(),
        url: repo.html_url.clone(),
        homepage: repo.homepage.clone().filter(|h| !h.trim().is_empty()),
        fork: repo.fork,
        archived: repo.archived,
        private: repo.private,
        stars: repo.stargazers_count,
        language: repo.language.clone(),
        topics: repo.topics.clone(),
        pushed_at: repo.pushed_at.clone(),
        release: rinfo,
        installed,
        icon,
        status,
        held,
        running,
        busy,
        has_previous,
    }
}

// ---------- Befehle ----------

#[tauri::command]
fn get_meta(suite: tauri::State<'_, Suite>) -> Meta {
    let appimage = std::env::var("APPIMAGE").ok().filter(|s| !s.is_empty());
    Meta {
        version: suite.own.pretty(),
        base_tag: suite.own.base_tag.clone(),
        owner: OWNER.into(),
        self_repo: SELF_REPO.into(),
        is_appimage: appimage.is_some(),
        appimage_path: appimage,
        settings: suite.settings.lock().unwrap().clone(),
        default_install_dir: store::default_install_dir(),
        arch: std::env::consts::ARCH.into(),
    }
}

#[tauri::command]
fn save_settings(suite: tauri::State<'_, Suite>, settings: Settings) -> Result<(), String> {
    let mut s = settings;
    if s.install_dir.trim().is_empty() {
        s.install_dir = store::default_install_dir();
    }
    store::save(&store::settings_file(), &s, true)?;
    *suite.settings.lock().unwrap() = s;
    Ok(())
}

async fn fetch_repos(suite: &Suite, token: &str) -> Result<Vec<Repo>, String> {
    let mut all: Vec<Repo> = Vec::new();
    for page in 1..=5 {
        let url = if token.is_empty() {
            format!("https://api.github.com/users/{OWNER}/repos?per_page=100&type=owner&sort=pushed&page={page}")
        } else {
            format!("https://api.github.com/user/repos?per_page=100&affiliation=owner&sort=pushed&page={page}")
        };
        let list: Vec<Repo> = suite.api.get_json(&url, token).await?.unwrap_or_default();
        let n = list.len();
        all.extend(list);
        if n < 100 {
            break;
        }
    }
    let owner = OWNER.to_lowercase();
    all.retain(|r| {
        let n = r.name.to_lowercase();
        r.owner.login.to_lowercase() == owner && n != SELF_REPO.to_lowercase() && n != owner && n != ".github"
    });
    Ok(all)
}

async fn latest_release(suite: &Suite, repo: &str, token: &str) -> Option<Release> {
    // Liste statt /latest: leere Antworten sind per ETag cachebar (304 zählt nicht gegen das Limit).
    let url = format!("https://api.github.com/repos/{OWNER}/{repo}/releases?per_page=10");
    let list: Vec<Release> = suite.api.get_json(&url, token).await.ok().flatten().unwrap_or_default();
    list.into_iter().find(|r| !r.draft && !r.prerelease)
}

async fn check_self(suite: &Suite, token: &str) -> Option<SelfUpdate> {
    if std::env::var("LAKUENSUITE_NO_UPDATE").is_ok() {
        return None;
    }
    let url = format!("https://api.github.com/repos/{OWNER}/{SELF_REPO}/releases/latest");
    let rel: Release = suite.api.get_json(&url, token).await.ok().flatten()?;
    let asset = github::pick_appimage(&rel.assets, Some("lakuensuite"))?;
    if !version::self_update_available(&suite.own, &rel.tag_name) {
        return None;
    }
    Some(SelfUpdate {
        tag: rel.tag_name.clone(),
        name: rel.name.clone().unwrap_or_else(|| rel.tag_name.clone()),
        body: rel.body.clone().unwrap_or_default(),
        url: rel.html_url.clone(),
        size: asset.size,
    })
}

#[tauri::command]
async fn list_apps(suite: tauri::State<'_, Suite>) -> Result<ListResult, String> {
    let token = suite.token();
    let mut error = None;
    let repos = match fetch_repos(&suite, &token).await {
        Ok(r) => r,
        Err(e) => {
            error = Some(e);
            // Mit dem zuletzt bekannten Stand weitermachen
            let known = suite.known.lock().unwrap();
            known.values().map(|(r, _)| r.clone()).collect()
        }
    };
    let futs = repos.iter().map(|r| latest_release(&suite, &r.name, &token));
    let releases = futures_util::future::join_all(futs).await;
    let self_update = check_self(&suite, &token).await;
    suite.api.save_cache(&store::http_cache_file());

    {
        let mut known = suite.known.lock().unwrap();
        known.clear();
        for (r, rel) in repos.iter().zip(releases.iter()) {
            known.insert(r.name.clone(), (r.clone(), rel.clone()));
        }
    }
    let mut apps: Vec<AppEntry> = repos.iter().zip(releases.iter()).map(|(r, rel)| build_entry(&suite, r, rel)).collect();

    // Installierte Apps, deren Repo nicht mehr gelistet ist, trotzdem anzeigen.
    let orphans: Vec<(String, Installed)> = {
        let st = suite.state.lock().unwrap();
        st.installed
            .iter()
            .filter(|(k, _)| !repos.iter().any(|r| &r.name == *k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    };
    for (name, _) in orphans {
        let repo = Repo {
            name: name.clone(),
            full_name: format!("{OWNER}/{name}"),
            html_url: format!("https://github.com/{OWNER}/{name}"),
            description: Some("Repo derzeit nicht erreichbar".into()),
            ..Default::default()
        };
        let mut e = build_entry(&suite, &repo, &None);
        e.status = "ok".into();
        apps.push(e);
    }

    Ok(ListResult {
        apps,
        self_update,
        offline: suite.api.offline.load(Ordering::Relaxed),
        rate_limited: suite.api.rate_limited.swap(false, Ordering::Relaxed),
        rate_remaining: suite.api.rate_remaining.load(Ordering::Relaxed),
        rate_reset: suite.api.rate_reset.load(Ordering::Relaxed),
        error,
    })
}

#[tauri::command]
async fn app_details(suite: tauri::State<'_, Suite>, repo: String) -> Result<Details, String> {
    let token = suite.token();
    let meta_url = format!("https://api.github.com/repos/{OWNER}/{repo}/contents/.lakuensuite.json");
    let readme_url = format!("https://api.github.com/repos/{OWNER}/{repo}/readme");
    let (meta, readme) = futures_util::future::join(
        suite.api.get(&meta_url, &token, "application/vnd.github.raw+json"),
        suite.api.get(&readme_url, &token, "application/vnd.github.raw+json"),
    )
    .await;
    suite.api.save_cache(&store::http_cache_file());
    let meta: SuiteFile = meta.ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let readme = readme.ok().flatten().and_then(|md| text::readme_excerpt(&md, 360));
    Ok(Details {
        name: meta.name,
        short: meta.short.map(|s| text::shorten(&s, 360)),
        category: meta.category,
        readme,
    })
}

struct BusyGuard<'a> {
    suite: &'a Suite,
    key: String,
}
impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.suite.busy.lock().unwrap().retain(|k| k != &self.key);
        self.suite.cancels.lock().unwrap().remove(&self.key);
    }
}

fn begin(suite: &Suite, key: &str) -> Result<(BusyGuard<'_>, Arc<AtomicBool>), String> {
    {
        let mut b = suite.busy.lock().unwrap();
        if b.iter().any(|k| k == key) {
            return Err("Läuft bereits".into());
        }
        b.push(key.to_string());
    }
    let flag = Arc::new(AtomicBool::new(false));
    suite.cancels.lock().unwrap().insert(key.to_string(), flag.clone());
    Ok((BusyGuard { suite, key: key.to_string() }, flag))
}

async fn download_verified(
    app: &AppHandle,
    suite: &Suite,
    key: &str,
    rel: &Release,
    asset: &Asset,
    dest: &Path,
    cancel: Arc<AtomicBool>,
) -> Result<(github::DlResult, bool), String> {
    let token = suite.token();
    emit(app, key, "prepare", 0, asset.size, 0, "Bereite Download vor …");
    let expected = github::expected_sha(&suite.api, rel, asset).await;
    let app2 = app.clone();
    let key2 = key.to_string();
    let res = github::download(&suite.api, asset, &token, dest, cancel, move |d, t, s| {
        emit(&app2, &key2, "download", d, t, s, "");
    })
    .await?;
    emit(app, key, "verify", res.size, res.size, 0, "Prüfe Datei …");
    let verified = match &expected {
        Some(h) => {
            if h != &res.sha256 {
                let _ = std::fs::remove_file(dest);
                return Err("Prüfsumme stimmt nicht – Datei verworfen. Bitte erneut versuchen.".into());
            }
            true
        }
        None => false,
    };
    if asset.size > 0 && res.size != asset.size {
        let _ = std::fs::remove_file(dest);
        return Err("Download unvollständig – bitte erneut versuchen.".into());
    }
    sysutil::make_executable(dest)?;
    Ok((res, verified))
}

#[tauri::command]
async fn install_app(app: AppHandle, suite: tauri::State<'_, Suite>, repo: String) -> Result<(), String> {
    let (repo_info, rel) = {
        let known = suite.known.lock().unwrap();
        match known.get(&repo) {
            Some((r, Some(rel))) => (r.clone(), rel.clone()),
            _ => return Err("Kein Release gefunden – bitte aktualisieren (F5).".into()),
        }
    };
    let asset = github::pick_appimage(&rel.assets, None)
        .cloned()
        .ok_or_else(|| "Dieses Release enthält kein AppImage für Linux.".to_string())?;
    let (install_dir, desktop, keep_prev) = {
        let s = suite.settings.lock().unwrap();
        (s.install_dir.clone(), s.desktop_entries, s.keep_previous)
    };
    let (_guard, cancel) = begin(&suite, &repo)?;
    let (dir, fin, prev, part) = app_paths(&install_dir, &repo);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Ordner {} nicht anlegbar: {e}", dir.display()))?;

    let result = download_verified(&app, &suite, &repo, &rel, &asset, &part, cancel).await;
    let (dl, verified) = match result {
        Ok(v) => v,
        Err(e) => {
            emit(&app, &repo, "error", 0, 0, 0, &e);
            return Err(e);
        }
    };
    emit(&app, &repo, "install", dl.size, dl.size, 0, "Installiere …");

    let old = suite.state.lock().unwrap().installed.get(&repo).cloned();
    if fin.exists() {
        if keep_prev {
            let _ = std::fs::rename(&fin, &prev);
        }
    }
    std::fs::rename(&part, &fin).map_err(|e| format!("Ersetzen fehlgeschlagen: {e}"))?;
    if !keep_prev {
        let _ = std::fs::remove_file(&prev);
    }

    emit(&app, &repo, "install", dl.size, dl.size, 0, "Hole Icon …");
    let fin2 = fin.clone();
    let repo2 = repo.clone();
    let icon = tauri::async_runtime::spawn_blocking(move || sysutil::extract_icon(&fin2, &repo2))
        .await
        .ok()
        .flatten()
        .or_else(|| old.as_ref().and_then(|o| o.icon.clone()).map(PathBuf::from).filter(|p| p.exists()));

    if desktop {
        let comment = repo_info.description.clone().unwrap_or_default();
        let _ = sysutil::write_desktop_entry(&repo, &repo, &comment, &fin, icon.as_deref());
    }

    let entry = Installed {
        tag: rel.tag_name.clone(),
        asset_id: asset.id,
        asset_name: asset.name.clone(),
        path: fin.to_string_lossy().to_string(),
        sha256: dl.sha256,
        verified,
        installed_at: store::now(),
        size: dl.size,
        prev_tag: if keep_prev { old.as_ref().map(|o| o.tag.clone()) } else { None },
        prev_asset_id: if keep_prev { old.as_ref().map(|o| o.asset_id) } else { None },
        hold_tag: None,
        icon: icon.map(|p| p.to_string_lossy().to_string()),
        launches: old.as_ref().map(|o| o.launches).unwrap_or(0),
        last_launch: old.as_ref().and_then(|o| o.last_launch),
    };
    suite.state.lock().unwrap().installed.insert(repo.clone(), entry);
    suite.save_state()?;
    let msg = if verified { "Fertig (Prüfsumme bestätigt)" } else { "Fertig (keine Prüfsumme im Release)" };
    emit(&app, &repo, "done", dl.size, dl.size, 0, msg);
    Ok(())
}

#[tauri::command]
fn cancel_download(suite: tauri::State<'_, Suite>, repo: String) {
    if let Some(f) = suite.cancels.lock().unwrap().get(&repo) {
        f.store(true, Ordering::Relaxed);
    }
}

#[tauri::command]
fn launch_app(suite: tauri::State<'_, Suite>, repo: String) -> Result<(), String> {
    let path = suite
        .state
        .lock()
        .unwrap()
        .installed
        .get(&repo)
        .map(|i| i.path.clone())
        .ok_or_else(|| "Nicht installiert".to_string())?;
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err("Die Datei wurde nicht gefunden – bitte neu installieren.".into());
    }
    let _ = sysutil::make_executable(&p);
    let child = sysutil::spawn_detached(&p, &[])?;
    suite.running.lock().unwrap().insert(repo.clone(), child);
    {
        let mut st = suite.state.lock().unwrap();
        if let Some(i) = st.installed.get_mut(&repo) {
            i.launches += 1;
            i.last_launch = Some(store::now());
        }
    }
    suite.save_state()
}

#[tauri::command]
fn running_apps(suite: tauri::State<'_, Suite>) -> Vec<String> {
    let mut r = suite.running.lock().unwrap();
    r.retain(|_, c| matches!(c.try_wait(), Ok(None)));
    r.keys().cloned().collect()
}

#[tauri::command]
fn uninstall_app(suite: tauri::State<'_, Suite>, repo: String) -> Result<(), String> {
    let inst = suite.state.lock().unwrap().installed.remove(&repo);
    if let Some(i) = inst {
        let p = PathBuf::from(&i.path);
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(p.with_file_name(format!("{repo}.prev.AppImage")));
        if let Some(icon) = &i.icon {
            let _ = std::fs::remove_file(icon);
        }
        if let Some(dir) = p.parent() {
            let _ = std::fs::remove_dir(dir); // nur wenn leer
        }
    }
    sysutil::remove_desktop_entry(&repo);
    suite.save_state()
}

#[tauri::command]
fn rollback_app(suite: tauri::State<'_, Suite>, repo: String) -> Result<String, String> {
    let mut st = suite.state.lock().unwrap();
    let i = st.installed.get_mut(&repo).ok_or("Nicht installiert")?;
    let (Some(prev_tag), Some(prev_id)) = (i.prev_tag.clone(), i.prev_asset_id) else {
        return Err("Keine vorherige Version vorhanden".into());
    };
    let cur = PathBuf::from(&i.path);
    let prev = cur.with_file_name(format!("{repo}.prev.AppImage"));
    if !prev.exists() {
        return Err("Die vorherige Version wurde nicht gefunden".into());
    }
    let tmp = cur.with_file_name(format!(".{repo}.swap"));
    std::fs::rename(&cur, &tmp).map_err(|e| e.to_string())?;
    std::fs::rename(&prev, &cur).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &prev).map_err(|e| e.to_string())?;
    let cur_tag = std::mem::replace(&mut i.tag, prev_tag.clone());
    let cur_id = std::mem::replace(&mut i.asset_id, prev_id);
    i.hold_tag = Some(cur_tag.clone());
    i.prev_tag = Some(cur_tag);
    i.prev_asset_id = Some(cur_id);
    i.verified = false;
    drop(st);
    suite.save_state()?;
    Ok(prev_tag)
}

#[tauri::command]
fn open_external(target: String) -> Result<(), String> {
    let ok = target.starts_with("https://") || Path::new(&target).exists();
    if !ok {
        return Err("Ziel nicht gefunden".into());
    }
    sysutil::open_external(&target)
}

#[tauri::command]
fn open_install_dir(suite: tauri::State<'_, Suite>, repo: Option<String>) -> Result<(), String> {
    let base = suite.settings.lock().unwrap().install_dir.clone();
    let mut p = PathBuf::from(base);
    if let Some(r) = repo {
        p = p.join(r);
    }
    std::fs::create_dir_all(&p).map_err(|e| e.to_string())?;
    sysutil::open_external(&p.to_string_lossy())
}

#[tauri::command]
fn set_desktop_entries(suite: tauri::State<'_, Suite>, enabled: bool) -> Result<u32, String> {
    let st = suite.state.lock().unwrap().clone();
    let mut n = 0;
    for (repo, i) in st.installed.iter() {
        if enabled {
            let icon = i.icon.as_ref().map(PathBuf::from);
            if sysutil::write_desktop_entry(repo, repo, "", Path::new(&i.path), icon.as_deref()).is_ok() {
                n += 1;
            }
        } else {
            sysutil::remove_desktop_entry(repo);
            n += 1;
        }
    }
    Ok(n)
}

#[tauri::command]
fn create_suite_shortcut() -> Result<String, String> {
    let path = std::env::var("APPIMAGE").map_err(|_| "Nur im AppImage möglich".to_string())?;
    let icon = sysutil::own_icon();
    let f = sysutil::write_desktop_entry(
        SELF_REPO,
        "LakuenSuite",
        "Alle Lakuen-Apps installieren, aktualisieren und starten",
        Path::new(&path),
        icon.as_deref(),
    )?;
    Ok(f.to_string_lossy().to_string())
}

#[tauri::command]
async fn self_update(app: AppHandle, suite: tauri::State<'_, Suite>) -> Result<(), String> {
    let path = std::env::var("APPIMAGE")
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "LakuenSuite läuft nicht als AppImage – bitte das Release manuell laden.".to_string())?;
    let token = suite.token();
    let url = format!("https://api.github.com/repos/{OWNER}/{SELF_REPO}/releases/latest");
    let rel: Release = suite
        .api
        .get_json(&url, &token)
        .await?
        .ok_or_else(|| "Kein Release von LakuenSuite gefunden".to_string())?;
    let asset = github::pick_appimage(&rel.assets, Some("lakuensuite"))
        .cloned()
        .ok_or_else(|| "Release ohne AppImage".to_string())?;
    let target = PathBuf::from(&path);
    let dir = target.parent().ok_or("Ungültiger Pfad")?.to_path_buf();
    let part = dir.join(".LakuenSuite.update.part");
    let (_guard, cancel) = begin(&suite, SELF_KEY)?;
    let (_dl, _verified) = match download_verified(&app, &suite, SELF_KEY, &rel, &asset, &part, cancel).await {
        Ok(v) => v,
        Err(e) => {
            emit(&app, SELF_KEY, "error", 0, 0, 0, &e);
            return Err(e);
        }
    };
    // Atomar ersetzen: Datei im selben Ordner, dann umbenennen. Laufende Instanz bleibt unberührt.
    std::fs::rename(&part, &target).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        format!("Konnte LakuenSuite nicht ersetzen (Schreibrechte für {}?): {e}", dir.display())
    })?;
    emit(&app, SELF_KEY, "done", 0, 0, 0, "Neustart …");
    sysutil::spawn_detached(&target, &[])?;
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        app2.exit(0);
    });
    Ok(())
}

fn main() {
    // Leeres/weißes Fenster unter manchen Mesa-/SteamOS-Konstellationen vermeiden.
    if std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").is_err() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        std::env::set_var("LAKUENSUITE_SET_DMABUF", "1");
    }
    let own = OwnVersion::from_describe(env!("LAKUEN_DESCRIBE"), env!("CARGO_PKG_VERSION"));
    let mut settings: Settings = store::load(&store::settings_file());
    if settings.install_dir.trim().is_empty() {
        settings.install_dir = store::default_install_dir();
    }
    let state: State = store::load(&store::state_file());
    let suite = Suite {
        api: Api::new(&format!("LakuenSuite/{}", own.base_tag), &store::http_cache_file()),
        own,
        settings: Mutex::new(settings),
        state: Mutex::new(state),
        cancels: Mutex::new(HashMap::new()),
        running: Mutex::new(HashMap::new()),
        known: Mutex::new(HashMap::new()),
        busy: Mutex::new(Vec::new()),
    };
    tauri::Builder::default()
        .manage(suite)
        .invoke_handler(tauri::generate_handler![
            get_meta,
            save_settings,
            list_apps,
            app_details,
            install_app,
            cancel_download,
            launch_app,
            running_apps,
            uninstall_app,
            rollback_app,
            open_external,
            open_install_dir,
            set_desktop_entries,
            create_suite_shortcut,
            self_update
        ])
        .run(tauri::generate_context!())
        .expect("LakuenSuite konnte nicht starten");
}
