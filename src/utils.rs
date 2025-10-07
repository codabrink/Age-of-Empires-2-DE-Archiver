use anyhow::{Result, bail};
use sevenz_rust2::ArchiveReader;
use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::PathBuf;
use zip::ZipArchive;

pub fn extract_7z(archive: &[u8]) -> Result<HashMap<String, Vec<u8>>> {
    let mut files = HashMap::new();

    let mut cursor = Cursor::new(archive);
    let mut archive = ArchiveReader::new(&mut cursor, "".into())?;

    archive.for_each_entries(|entry, reader| {
        let mut content = vec![];
        reader.read_to_end(&mut content);
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
