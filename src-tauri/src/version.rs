//! Versionslogik ohne externe Abhängigkeiten (lokal mit `rustc --test` prüfbar).

use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ver {
    pub nums: [u64; 3],
    /// Vorabkennung, z. B. "test7" aus "v0.3.3-test7". Eine Vorabversion ist älter als die Version ohne.
    pub pre: Option<String>,
}

impl Ver {
    pub fn parse(tag: &str) -> Option<Ver> {
        let t = tag.trim();
        let t = t.strip_prefix('v').or_else(|| t.strip_prefix('V')).unwrap_or(t);
        let (core, pre) = match t.find(|c| c == '-' || c == '+' || c == ' ') {
            Some(i) => (&t[..i], Some(t[i + 1..].trim().to_string()).filter(|s| !s.is_empty())),
            None => (t, None),
        };
        let mut nums = [0u64; 3];
        let parts: Vec<&str> = core.split('.').collect();
        if parts.is_empty() || parts.len() > 3 {
            return None;
        }
        for (i, p) in parts.iter().enumerate() {
            nums[i] = p.parse().ok()?;
        }
        Some(Ver { nums, pre })
    }
}

impl PartialOrd for Ver {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Ver {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.nums.cmp(&other.nums) {
            Ordering::Equal => match (&self.pre, &other.pre) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => a.cmp(b),
            },
            o => o,
        }
    }
}

/// Eigene Version, aus `git describe --tags --long --always` beim Bauen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnVersion {
    /// Letzter Tag, z. B. "v0.1.0" (oder Paketversion, wenn kein Tag existiert).
    pub base_tag: String,
    /// Commits seit dem Tag (0 = genau das Release).
    pub commits: u32,
    pub hash: Option<String>,
}

impl OwnVersion {
    pub fn from_describe(describe: &str, pkg_version: &str) -> OwnVersion {
        let d = describe.trim();
        // Format: <tag>-<n>-g<hash>; der Tag selbst darf Bindestriche enthalten.
        let parts: Vec<&str> = d.rsplitn(3, '-').collect();
        if parts.len() == 3 && parts[0].starts_with('g') {
            if let Ok(n) = parts[1].parse::<u32>() {
                return OwnVersion {
                    base_tag: parts[2].to_string(),
                    commits: n,
                    hash: Some(parts[0][1..].to_string()),
                };
            }
        }
        let is_hash = !d.is_empty() && d.len() <= 40 && d.chars().all(|c| c.is_ascii_hexdigit());
        OwnVersion {
            base_tag: format!("v{}", pkg_version),
            commits: if is_hash { 1 } else { 0 },
            hash: if is_hash { Some(d.to_string()) } else { None },
        }
    }

    /// Lesbar: "v0.3.3" oder "v0.3.3 test 7 (1f535a3)".
    pub fn pretty(&self) -> String {
        if self.commits == 0 {
            self.base_tag.clone()
        } else {
            match &self.hash {
                Some(h) => format!("{} test {} ({})", self.base_tag, self.commits, h),
                None => format!("{} test {}", self.base_tag, self.commits),
            }
        }
    }
}

/// Soll LakuenSuite sich selbst aktualisieren?
pub fn self_update_available(own: &OwnVersion, latest_tag: &str) -> bool {
    match (Ver::parse(&own.base_tag), Ver::parse(latest_tag)) {
        (Some(mine), Some(latest)) => latest > mine,
        // Nicht vergleichbar: nur wenn der Tag sich unterscheidet.
        _ => latest_tag.trim() != own.base_tag.trim(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppState {
    NotInstalled,
    UpToDate,
    Update,
}

/// Vergleich installiert vs. neuestes Release. Jede Abweichung (Tag oder neu
/// hochgeladene Datei bei gleichem Tag) heißt „Update“.
pub fn app_state(installed: Option<(&str, u64)>, latest: (&str, u64)) -> AppState {
    match installed {
        None => AppState::NotInstalled,
        Some((tag, asset)) => {
            if tag.trim() != latest.0.trim() || asset != latest.1 {
                AppState::Update
            } else {
                AppState::UpToDate
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_order() {
        assert!(Ver::parse("v0.2.0").unwrap() > Ver::parse("v0.1.9").unwrap());
        assert!(Ver::parse("0.10.0").unwrap() > Ver::parse("v0.9.0").unwrap());
        assert!(Ver::parse("v1.0").unwrap() == Ver::parse("1.0.0").unwrap());
        assert!(Ver::parse("v0.3.3-test7").unwrap() < Ver::parse("v0.3.3").unwrap());
        assert!(Ver::parse("nightly").is_none());
    }

    #[test]
    fn describe_pretty() {
        let a = OwnVersion::from_describe("v0.1.0-0-gabc1234", "0.1.0");
        assert_eq!(a.pretty(), "v0.1.0");
        let b = OwnVersion::from_describe("v0.3.3-7-g1f535a3", "0.3.3");
        assert_eq!(b.pretty(), "v0.3.3 test 7 (1f535a3)");
        let c = OwnVersion::from_describe("1f535a3", "0.1.0");
        assert_eq!(c.pretty(), "v0.1.0 test 1 (1f535a3)");
        let d = OwnVersion::from_describe("", "0.1.0");
        assert_eq!(d.pretty(), "v0.1.0");
    }

    #[test]
    fn self_update_decision() {
        let rel = OwnVersion::from_describe("v0.2.0-0-gaaaaaaa", "0.2.0");
        assert!(!self_update_available(&rel, "v0.1.0")); // älter
        assert!(!self_update_available(&rel, "v0.2.0")); // gleich
        assert!(self_update_available(&rel, "v0.2.1")); // neuer
        // Testbuild auf einem Release: kein Rückschritt aufs Release
        let test = OwnVersion::from_describe("v0.2.0-5-gbbbbbbb", "0.2.0");
        assert!(!self_update_available(&test, "v0.2.0"));
        assert!(self_update_available(&test, "v0.2.1"));
    }

    #[test]
    fn app_decision() {
        assert_eq!(app_state(None, ("v1.0.0", 1)), AppState::NotInstalled);
        assert_eq!(app_state(Some(("v1.0.0", 1)), ("v1.0.0", 1)), AppState::UpToDate);
        assert_eq!(app_state(Some(("v0.9.0", 1)), ("v1.0.0", 2)), AppState::Update);
        // gleicher Tag, Datei neu hochgeladen (Testbuild ersetzt)
        assert_eq!(app_state(Some(("v1.0.0", 1)), ("v1.0.0", 5)), AppState::Update);
        // anders als das Release (z. B. zurückgezogen) -> trotzdem Update
        assert_eq!(app_state(Some(("v1.1.0", 1)), ("v1.0.0", 1)), AppState::Update);
    }
}
