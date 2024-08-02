use std::{io::Error, path::PathBuf};
use serde::Deserialize;
use once_cell::sync::OnceCell;

static EXTENSION_LIST: OnceCell<Vec<ExtensionInfo>> = OnceCell::new();

#[allow(non_snake_case)]
#[derive(Debug, Deserialize, Clone)]
pub struct SourceInfo {
    pub name: String,
    pub lang: String,
    pub id: String,
    pub baseUrl: String,
}

impl Default for SourceInfo {
    fn default() -> Self {
        SourceInfo {
            name: String::from("Unknown"), 
            lang: String::from("en"), 
            id: 0.to_string(), 
            baseUrl: String::from("example.com")
        }
    }
}

#[allow(non_snake_case)]
#[derive(Debug, Deserialize)]
pub struct ExtensionInfo {
    pub name: String,
    pub pkg: String,
    pub apk: String,
    pub lang: String,
    pub code: i32,
    pub version: String,
    pub nsfw: i32,
    pub sources: Vec<SourceInfo>
}

pub fn get_source(id: i64) -> std::io::Result<SourceInfo> {
    let id = id.to_string();
    let extensions = EXTENSION_LIST.get_or_try_init(|| {
        let path = crate::PROJECT_DIR.data_dir();
        if !path.try_exists()? {
            std::fs::create_dir_all(path)?;
        }
        let tachi_path = PathBuf::from(path).join("tachi_sources.json");
        let extensions = std::fs::read_to_string(tachi_path)
        .map_err(|_e| {
            Error::new(
                std::io::ErrorKind::NotFound,
                "Extension info missing; run `nekotatsu update` to update list"
            )
        })?;
        let extensions: Vec<ExtensionInfo> = serde_json::from_str(&extensions)?;
        std::io::Result::Ok(extensions)
    })?;

    Ok(extensions.iter().flat_map(|extension| &extension.sources)
            .find(|source| source.id == id)
            .map(|s| s.clone())
            .unwrap_or(SourceInfo {
                id,
                ..Default::default()
            }))
}