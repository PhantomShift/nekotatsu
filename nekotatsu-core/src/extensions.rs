use serde::Deserialize;
use std::io::Read;

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
            baseUrl: String::from("example.com"),
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
    pub sources: Vec<SourceInfo>,
}

#[derive(Debug)]
pub struct ExtensionList {
    inner: Vec<ExtensionInfo>,
}

impl Default for ExtensionList {
    fn default() -> Self {
        Self { inner: Vec::new() }
    }
}

impl ExtensionList {
    pub fn new(list: Vec<ExtensionInfo>) -> Self {
        Self { inner: list }
    }

    pub fn try_from_file(mut file: std::fs::File) -> std::io::Result<Self> {
        let mut extensions = String::new();
        file.read_to_string(&mut extensions)?;
        Ok(Self {
            inner: serde_json::from_str(&extensions)?,
        })
    }

    pub fn get_source(&self, id: i64) -> Option<SourceInfo> {
        let id = id.to_string();
        self.inner
            .iter()
            .flat_map(|e| &e.sources)
            .find(|s| s.id == id)
            .map(|s| s.clone())
    }
}
