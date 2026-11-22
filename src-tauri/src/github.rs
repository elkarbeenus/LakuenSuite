//! GitHub-API mit ETag-Cache: unveränderte Antworten (304) zählen nicht gegen das Limit,
//! und ohne Netz wird der letzte Stand angezeigt.

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;

use crate::text;

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Owner {
    pub login: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Repo {
    pub name: String,
    pub full_name: String,
    pub owner: Owner,
    pub description: Option<String>,
    pub html_url: String,
    pub homepage: Option<String>,
    pub fork: bool,
    pub archived: bool,
    pub private: bool,
    pub pushed_at: Option<String>,
    pub updated_at: Option<String>,
    pub stargazers_count: u64,
    pub language: Option<String>,
    pub topics: Vec<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Asset {
    pub id: u64,
    pub name: String,
    pub size: u64,
    pub browser_download_url: String,
    pub url: String,
    pub digest: Option<String>,
    pub updated_at: Option<String>,
    pub download_count: u64,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Release {
    pub tag_name: String,
    pub name: Option<String>,
    pub body: Option<String>,
    pub html_url: String,
    pub published_at: Option<String>,
    pub draft: bool,
    pub prerelease: bool,
    pub assets: Vec<Asset>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Cached {
    etag: Option<String>,
    body: String,
}

pub struct Api {
    pub client: reqwest::Client,
    cache: Mutex<HashMap<String, Cached>>,
    pub rate_remaining: AtomicI64,
    pub rate_reset: AtomicI64,
    pub offline: AtomicBool,
    pub rate_limited: AtomicBool,
}

impl Api {
    pub fn new(user_agent: &str, cache_file: &Path) -> Api {
        let client = reqwest::Client::builder()
            .user_agent(user_agent)
            .connect_timeout(Duration::from_secs(12))
            .build()
            .expect("HTTP-Client");
        let cache: HashMap<String, Cached> = std::fs::read_to_string(cache_file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Api {
            client,
            cache: Mutex::new(cache),
            rate_remaining: AtomicI64::new(-1),
            rate_reset: AtomicI64::new(0),
            offline: AtomicBool::new(false),
            rate_limited: AtomicBool::new(false),
        }
    }

    pub fn save_cache(&self, cache_file: &Path) {
        let data = {
            let c = self.cache.lock().unwrap();
            serde_json::to_string(&*c).unwrap_or_default()
        };
        if let Some(d) = cache_file.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(cache_file, data);
    }

    /// GET mit Cache. `Ok(None)` = 404.
    pub async fn get(&self, url: &str, token: &str, accept: &str) -> Result<Option<String>, String> {
        let cached = { self.cache.lock().unwrap().get(url).cloned() };
        let mut req = self
            .client
            .get(url)
            .header("Accept", accept)
            .header("X-GitHub-Api-Version", "2022-11-28")
            .timeout(Duration::from_secs(25));
        if !token.is_empty() {
            req = req.bearer_auth(token);
        }
        if let Some(e) = cached.as_ref().and_then(|c| c.etag.clone()) {
            req = req.header("If-None-Match", e);
        }
        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                self.offline.store(true, Ordering::Relaxed);
                return match cached {
                    Some(c) => Ok(Some(c.body)),
                    None => Err(format!("Keine Verbindung zu GitHub ({e})")),
                };
            }
        };
        self.offline.store(false, Ordering::Relaxed);
        let h = resp.headers();
        if let Some(v) = h.get("x-ratelimit-remaining").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()) {
            self.rate_remaining.store(v, Ordering::Relaxed);
        }
        if let Some(v) = h.get("x-ratelimit-reset").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()) {
            self.rate_reset.store(v, Ordering::Relaxed);
        }
        let status = resp.status().as_u16();
        if status == 304 {
            return Ok(cached.map(|c| c.body));
        }
        if status == 404 {
            self.cache.lock().unwrap().remove(url);
            return Ok(None);
        }
        if status == 403 || status == 429 {
            self.rate_limited.store(true, Ordering::Relaxed);
            return match cached {
                Some(c) => Ok(Some(c.body)),
                None => Err("GitHub-Abfragelimit erreicht. Später erneut versuchen oder in den Einstellungen einen GitHub-Token eintragen.".into()),
            };
        }
        if status == 401 {
            return Err("GitHub-Token ungültig. Bitte in den Einstellungen prüfen.".into());
        }
        if !(200..300).contains(&status) {
            return match cached {
                Some(c) => Ok(Some(c.body)),
                None => Err(format!("GitHub antwortet mit Fehler {status}")),
            };
        }
        let etag = resp.headers().get("etag").and_then(|v| v.to_str().ok()).map(String::from);
        let body = resp.text().await.map_err(|e| e.to_string())?;
        self.cache.lock().unwrap().insert(url.to_string(), Cached { etag, body: body.clone() });
        Ok(Some(body))
    }

    pub async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str, token: &str) -> Result<Option<T>, String> {
        match self.get(url, token, "application/vnd.github+json").await? {
            None => Ok(None),
            Some(body) => serde_json::from_str(&body).map(Some).map_err(|e| format!("Antwort nicht lesbar: {e}")),
        }
    }

    /// Einfache Textdatei laden (z. B. Prüfsummen), ohne Cache.
    pub async fn get_text(&self, url: &str) -> Result<String, String> {
        let r = self
            .client
            .get(url)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !r.status().is_success() {
            return Err(format!("Fehler {}", r.status().as_u16()));
        }
        r.text().await.map_err(|e| e.to_string())
    }
}

/// Passendes AppImage für diese Architektur wählen.
pub fn pick_appimage<'a>(assets: &'a [Asset], prefer: Option<&str>) -> Option<&'a Asset> {
    let (mine, other): (&[&str], &[&str]) = if std::env::consts::ARCH == "aarch64" {
        (&["aarch64", "arm64"], &["x86_64", "amd64", "x64", "i386", "i686", "armhf"])
    } else {
        (&["x86_64", "amd64", "x64"], &["aarch64", "arm64", "armhf", "armv7", "i386", "i686"])
    };
    let cands: Vec<&Asset> = assets
        .iter()
        .filter(|a| a.name.to_lowercase().ends_with(".appimage"))
        .filter(|a| {
            let n = a.name.to_lowercase();
            mine.iter().any(|m| n.contains(m)) || !other.iter().any(|o| n.contains(o))
        })
        .collect();
    if let Some(p) = prefer {
        let p = p.to_lowercase();
        if let Some(a) = cands.iter().find(|a| a.name.to_lowercase().contains(&p)) {
            return Some(a);
        }
    }
    cands
        .iter()
        .find(|a| {
            let n = a.name.to_lowercase();
            mine.iter().any(|m| n.contains(m))
        })
        .or_else(|| cands.first())
        .copied()
}

/// Erwarteten SHA-256 ermitteln: GitHub-Digest, sonst .sha256-/SHA256SUMS-Datei im Release.
pub async fn expected_sha(api: &Api, release: &Release, asset: &Asset) -> Option<String> {
    if let Some(d) = &asset.digest {
        if let Some(h) = d.strip_prefix("sha256:") {
            return Some(h.to_lowercase());
        }
    }
    let lname = asset.name.to_lowercase();
    let side = release.assets.iter().find(|a| {
        let n = a.name.to_lowercase();
        n == format!("{lname}.sha256") || n == format!("{lname}.sha256sum") || n == format!("{lname}.sha256.txt")
    });
    let sums = side.or_else(|| {
        release.assets.iter().find(|a| {
            let n = a.name.to_lowercase();
            n.starts_with("sha256sums") || n == "checksums.txt" || n == "sha256.txt" || n.ends_with("_checksums.txt")
        })
    })?;
    let text = api.get_text(&sums.browser_download_url).await.ok()?;
    text::parse_checksum(&text, &asset.name)
}

pub struct DlResult {
    pub sha256: String,
    pub size: u64,
}

/// Datei herunterladen, dabei SHA-256 berechnen. Fortschritt über `on_progress(geladen, gesamt, bytes_pro_s)`.
pub async fn download(
    api: &Api,
    asset: &Asset,
    token: &str,
    dest: &Path,
    cancel: Arc<AtomicBool>,
    mut on_progress: impl FnMut(u64, u64, u64),
) -> Result<DlResult, String> {
    let mut req = if token.is_empty() {
        api.client.get(&asset.browser_download_url)
    } else {
        // Über die API: funktioniert auch für private Repos.
        api.client
            .get(&asset.url)
            .header("Accept", "application/octet-stream")
            .bearer_auth(token)
    };
    req = req.header("X-GitHub-Api-Version", "2022-11-28");
    let resp = req.send().await.map_err(|e| format!("Download fehlgeschlagen: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Download fehlgeschlagen (Fehler {})", resp.status().as_u16()));
    }
    let total = resp.content_length().unwrap_or(asset.size);
    if let Some(d) = dest.parent() {
        tokio::fs::create_dir_all(d).await.map_err(|e| e.to_string())?;
    }
    let mut file = tokio::fs::File::create(dest).await.map_err(|e| format!("Kann Datei nicht anlegen: {e}"))?;
    let mut hasher = Sha256::new();
    let mut done: u64 = 0;
    let mut stream = resp.bytes_stream();
    let start = Instant::now();
    let mut last: Option<Instant> = None;
    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::Relaxed) {
            drop(file);
            let _ = tokio::fs::remove_file(dest).await;
            return Err("Abgebrochen".into());
        }
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                drop(file);
                let _ = tokio::fs::remove_file(dest).await;
                return Err(format!("Verbindung abgebrochen: {e}"));
            }
        };
        hasher.update(&chunk);
        if let Err(e) = file.write_all(&chunk).await {
            drop(file);
            let _ = tokio::fs::remove_file(dest).await;
            return Err(format!("Schreiben fehlgeschlagen (Speicher voll?): {e}"));
        }
        done += chunk.len() as u64;
        if last.map_or(true, |l| l.elapsed() >= Duration::from_millis(150)) {
            last = Some(Instant::now());
            let secs = start.elapsed().as_secs_f64().max(0.001);
            on_progress(done, total, (done as f64 / secs) as u64);
        }
    }
    file.flush().await.map_err(|e| e.to_string())?;
    file.sync_all().await.map_err(|e| e.to_string())?;
    drop(file);
    let secs = start.elapsed().as_secs_f64().max(0.001);
    on_progress(done, total.max(done), (done as f64 / secs) as u64);
    let sha256 = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>();
    Ok(DlResult { sha256, size: done })
}
