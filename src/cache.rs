use crate::{config::Config, models::Service, scanner};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Serialize, Deserialize)]
struct CacheFile {
    fetched_at: u64,
    config_signature: String,
    services: Vec<Service>,
}

pub fn services_for_waybar(config: &Config) -> Vec<Service> {
    let Some(path) = cache_path() else {
        return scanner::discover_services(config);
    };
    let ttl = config.refresh_interval.clamp(1, 60);
    let signature = config_signature(config);
    if let Some(cached) = read_fresh(&path, ttl, now(), &signature) {
        return cached;
    }
    let services = scanner::discover_services(config);
    let data = CacheFile {
        fetched_at: now(),
        config_signature: signature,
        services: services.clone(),
    };
    if let Ok(serialized) = serde_json::to_vec(&data) {
        let _ = write_atomically(&path, &serialized);
    }
    services
}

fn cache_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    Some(base.join("portpilot/services.json"))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn read_fresh(path: &Path, ttl: u64, now: u64, signature: &str) -> Option<Vec<Service>> {
    let contents = fs::read(path).ok()?;
    let cache: CacheFile = serde_json::from_slice(&contents).ok()?;
    (cache.config_signature == signature && fresh_at(cache.fetched_at, ttl, now))
        .then_some(cache.services)
}

fn config_signature(config: &Config) -> String {
    format!(
        "{}:{:?}",
        config.projects.search_git_root, config.ports.ignore
    )
}

fn write_atomically(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let directory = path
        .parent()
        .expect("cache file always has a parent directory");
    fs::create_dir_all(directory)?;
    #[cfg(unix)]
    fs::set_permissions(
        directory,
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )?;

    let temporary = directory.join(format!("services.{}.tmp", std::process::id()));
    fs::write(&temporary, data)?;
    #[cfg(unix)]
    fs::set_permissions(
        &temporary,
        std::os::unix::fs::PermissionsExt::from_mode(0o600),
    )?;
    fs::rename(temporary, path)
}

fn fresh_at(fetched_at: u64, ttl: u64, now: u64) -> bool {
    now >= fetched_at && now - fetched_at < ttl
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn accepts_cache_within_ttl_and_rejects_stale_cache() {
        assert!(fresh_at(100, 3, 102));
        assert!(!fresh_at(100, 3, 103));
        assert!(!fresh_at(0, 3, u64::MAX));
    }

    #[test]
    fn cache_round_trips_empty_service_list() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("services.json");
        let contents = serde_json::to_vec(&CacheFile {
            fetched_at: 22,
            config_signature: "default".into(),
            services: Vec::new(),
        })
        .unwrap();
        fs::write(&path, contents).unwrap();
        assert!(read_fresh(&path, 3, 24, "default").unwrap().is_empty());
        assert!(read_fresh(&path, 3, 26, "default").is_none());
        assert!(read_fresh(&path, 3, 24, "different-config").is_none());
    }
}
