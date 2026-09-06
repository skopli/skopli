//! Home-directory and cache-directory resolution shared by the core (built-in
//! pricing sources) and by every binding's `default_cache_dir` entry point.
//! Faithful port of `homeDir` / `defaultCacheDir` from `src/pricing/index.ts`
//! (platform-specific XDG/AppData/Library layout).

use std::path::PathBuf;

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// The current user's home directory: `HOME` on Unix, `USERPROFILE` (or
/// `HOMEDRIVE`+`HOMEPATH`) on Windows. Empty string when unknown.
pub fn home_dir() -> String {
    if cfg!(windows) {
        if let Some(p) = env_nonempty("USERPROFILE") {
            return p;
        }
        if let (Some(d), Some(p)) = (env_nonempty("HOMEDRIVE"), env_nonempty("HOMEPATH")) {
            return format!("{d}{p}");
        }
        String::new()
    } else {
        env_nonempty("HOME").unwrap_or_default()
    }
}

/// The default on-disk pricing cache directory (platform-specific
/// XDG/AppData/Library layout). Faithful port of `defaultCacheDir`.
pub fn default_cache_dir() -> String {
    let home = home_dir();
    let join = |parts: &[&str]| {
        let mut p = PathBuf::from(&home);
        for part in parts {
            p.push(part);
        }
        p.to_string_lossy().into_owned()
    };
    if cfg!(windows) {
        if let Some(local) = env_nonempty("LOCALAPPDATA") {
            let mut p = PathBuf::from(local);
            p.push("skopli");
            p.push("cache");
            return p.to_string_lossy().into_owned();
        }
        return join(&["AppData", "Local", "skopli", "cache"]);
    }
    if let Some(xdg) = env_nonempty("XDG_CACHE_HOME") {
        let mut p = PathBuf::from(xdg);
        p.push("skopli");
        return p.to_string_lossy().into_owned();
    }
    if cfg!(target_os = "macos") {
        return join(&["Library", "Caches", "skopli"]);
    }
    join(&[".cache", "skopli"])
}
