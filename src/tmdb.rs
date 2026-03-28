use anyhow::{Context, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use crate::config::Config;
use crate::types::{Episode, TmdbMovie, TmdbShow};

static TMDB_AGENT: LazyLock<ureq::Agent> = LazyLock::new(|| {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into()
});

fn config_path() -> PathBuf {
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"));
    home.join(".config").join("bluback").join("tmdb_api_key")
}

pub fn get_api_key(config: &Config) -> Option<String> {
    config.tmdb_api_key()
}

pub fn save_api_key(key: &str) -> Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, format!("{}\n", key))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

fn urlencoding(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                String::from(b as char)
            }
            _ => format!("%{:02X}", b),
        })
        .collect()
}

fn tmdb_get(path: &str, api_key: &str, extra_params: &[(&str, &str)]) -> Result<serde_json::Value> {
    if extra_params.is_empty() {
        log::debug!("TMDb request: {}", path);
    } else {
        let params: Vec<String> = extra_params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        log::debug!("TMDb request: {}?{}", path, params.join("&"));
    }

    let mut url = format!("https://api.themoviedb.org/3{}?api_key={}", path, api_key);
    for (k, v) in extra_params {
        url.push('&');
        url.push_str(k);
        url.push('=');
        url.push_str(&urlencoding(v));
    }

    TMDB_AGENT
        .get(&url)
        .header("Accept", "application/json")
        .call()
        .context("TMDb request failed")?
        .body_mut()
        .read_json()
        .context("Failed to parse TMDb response")
}

pub fn search_show(query: &str, api_key: &str) -> Result<Vec<TmdbShow>> {
    let data = tmdb_get("/search/tv", api_key, &[("query", query)])?;
    let results: Vec<TmdbShow> =
        serde_json::from_value(data.get("results").cloned().unwrap_or_default())?;
    Ok(results)
}

pub fn search_movie(query: &str, api_key: &str) -> Result<Vec<TmdbMovie>> {
    let data = tmdb_get("/search/movie", api_key, &[("query", query)])?;
    let results: Vec<TmdbMovie> =
        serde_json::from_value(data.get("results").cloned().unwrap_or_default())?;
    Ok(results)
}

pub fn get_season(show_id: u64, season: u32, api_key: &str) -> Result<Vec<Episode>> {
    let path = format!("/tv/{}/season/{}", show_id, season);
    let data = tmdb_get(&path, api_key, &[])?;
    let episodes: Vec<Episode> =
        serde_json::from_value(data.get("episodes").cloned().unwrap_or_default())?;
    Ok(episodes)
}
