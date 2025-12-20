use anyhow::{Result, bail};
use serde_json::Value;
use sevenz_rust2::ArchiveReader;
use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use zip::ZipArchive;

pub fn extract_7z(archive: &[u8]) -> Result<HashMap<String, Vec<u8>>> {
    let mut files = HashMap::new();

    let mut cursor = Cursor::new(archive);
    let mut archive = ArchiveReader::new(&mut cursor, "".into())?;

    archive.for_each_entries(|entry, reader| {
        let mut content = vec![];
        let _ = reader.read_to_end(&mut content);
        files.insert(entry.name.clone(), content);
        Ok(true)
    })?;

    Ok(files)
}

pub fn extract_zip(data: &[u8]) -> Result<HashMap<String, Vec<u8>>> {
    let reader = Cursor::new(data);
    let mut archive = ZipArchive::new(reader)?;
    let mut map = HashMap::new();

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;
        map.insert(file.name().to_string(), contents);
    }

    Ok(map)
}

pub fn desktop_dir() -> Result<PathBuf> {
    let Some(desktop_dir) = dirs::desktop_dir() else {
        bail!("Missing desktop dir.");
    };
    Ok(desktop_dir)
}

pub fn gh_latest_release_dl_url(
    gh_user: &str,
    gh_repo: &str,
    search: &[&str],
) -> Result<Option<String>> {
    let url = format!("https://api.github.com/repos/{gh_user}/{gh_repo}/releases/latest",);

    // Ask the api for the latest release download
    let client = reqwest::blocking::Client::new();
    let json = client
        .get(url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:143.0) Gecko/20100101 Firefox/143.0",
        )
        .send()?
        .text()?;
    let json: Value = serde_json::from_str(&json)?;

    let Some(assets) = json.get("assets") else {
        bail!("Unexpected response from github: expected assets field.");
    };
    let Some(assets) = assets.as_array() else {
        bail!("Expected github assets to be an array, but it was not.");
    };

    for asset in assets {
        let Some(name) = asset.get("name").and_then(|n| n.as_str()) else {
            continue;
        };

        if !search.iter().all(|s| name.contains(s)) {
            continue;
        }

        let Some(url) = asset.get("browser_download_url").and_then(|u| u.as_str()) else {
            continue;
        };

        return Ok(Some(url.to_string()));
    }

    Ok(None)
}

pub struct Busy {
    busy: Arc<AtomicBool>,
}
pub struct BusyGuard {
    busy: Arc<AtomicBool>,
}
impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.busy.store(false, Ordering::SeqCst);
    }
}

impl Busy {
    pub fn new() -> Self {
        Self {
            busy: Arc::default(),
        }
    }

    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Relaxed)
    }

    pub fn lock(&self) -> Result<BusyGuard> {
        if self.busy.swap(true, Ordering::SeqCst) {
            bail!("Already busy.");
        }
        Ok(BusyGuard {
            busy: self.busy.clone(),
        })
    }
}
