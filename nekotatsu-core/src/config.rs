use crate::SourceInfo;
use serde::{de::Visitor, Deserialize};

#[derive(Debug, PartialEq, Eq)]
pub enum SourceFilterEntry {
    Id(i64),
    Name(String),
    Url(String),
}

struct SourceFilterEntryVisitor;
impl<'de> Visitor<'de> for SourceFilterEntryVisitor {
    type Value = SourceFilterEntry;
    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a number id, source name or source url")
    }

    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(SourceFilterEntry::Id(v))
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        if v.contains('.') {
            return Ok(SourceFilterEntry::Url(v.to_ascii_lowercase()));
        }
        Ok(SourceFilterEntry::Name(v.to_lowercase()))
    }
}

impl<'de> Deserialize<'de> for SourceFilterEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = deserializer.deserialize_any(SourceFilterEntryVisitor)?;
        Ok(value)
    }
}

#[derive(Debug, Deserialize)]
pub struct ConfigFile {
    pub whitelist: Option<Vec<SourceFilterEntry>>,
    pub blacklist: Option<Vec<SourceFilterEntry>>,
}

impl Default for ConfigFile {
    fn default() -> Self {
        ConfigFile {
            whitelist: None,
            blacklist: None,
        }
    }
}

#[test]
fn parse_config() -> Result<(), Box<dyn std::error::Error>> {
    let config = r#"
whitelist = [
    1252145125,
    "mangadex",
    "toomic.com/en"
]

blacklist = [
    4201337,
    "mangasomething",
    "my.manga.me"
]"#;
    let config: ConfigFile = toml::from_str(config)?;

    println!("{config:?}");

    Ok(())
}

pub trait SourceFilterList {
    /// Check if list has an item;
    /// falls back to `default` if list is empty
    fn check_source(&self, default: bool, source: &SourceInfo) -> bool;
}

impl SourceFilterList for Vec<SourceFilterEntry> {
    fn check_source(&self, default: bool, source: &SourceInfo) -> bool {
        if self.is_empty() {
            return default;
        }
        self.contains(&SourceFilterEntry::Id(
            source.id.parse::<i64>().expect("should be int"),
        )) || self.contains(&SourceFilterEntry::Name(source.name.to_lowercase()))
            || self.contains(&SourceFilterEntry::Url(source.baseUrl.to_lowercase()))
    }
}
