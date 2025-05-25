use std::{
    fs::File,
    io::{BufReader, Cursor, Read, Write},
    sync::LazyLock,
};

use regex::Regex;
use serde::{Deserialize, Serialize};
use tracing::warn;
use zip::ZipArchive;

enum DomainCaptureMethod {
    Single(Regex),
    Multiple(Regex),
}

impl DomainCaptureMethod {
    fn capture_domains(&self, subject: &str) -> Option<Vec<String>> {
        match self {
            DomainCaptureMethod::Single(r) => {
                if let Some(captures) = r.captures(subject) {
                    return Some(vec![captures["domain"].to_string()]);
                }
                None
            }
            DomainCaptureMethod::Multiple(r) => {
                if let Some(captures) = r.captures(subject) {
                    let list = &captures["domains"];
                    return Some(
                        list.split(",")
                            .map(|s| s.replace('"', "").replace(&[' ', '\t', '\n', '\r'], ""))
                            .filter(|s| !s.is_empty())
                            .collect(),
                    );
                }
                None
            }
        }
    }
}

static PARSER_CAPTURE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"@MangaSourceParser\(.(?P<name>\w*)., .(?P<title>[\w\s\(\)]+).(, .(?P<locale>\w*).(, (?P<type>[\w\.]+))?)?"#).unwrap()
});

static DOMAIN_CAPTURE_METHODS: LazyLock<Vec<DomainCaptureMethod>> = LazyLock::new(|| {
    vec![
    DomainCaptureMethod::Multiple(regex::RegexBuilder::new(r#"ConfigKey\.Domain\((?P<domains>.+?)\)"#)
        .dot_matches_new_line(true)
        .build()
        .unwrap()),
    DomainCaptureMethod::Single(Regex::new(r#"\w+\(\s*context,\s*\w+Source\.\w+,\s*"(?P<domain>[\w\.\-/]+)""#).unwrap()),
    DomainCaptureMethod::Single(Regex::new(r#"\(\s*context,\s*MangaSource\.\w+,\s*.(?P<domain>[\w\.\-/]+)."#).unwrap()),
    DomainCaptureMethod::Single(regex::RegexBuilder::new(r#"\w+\(\s*context = context,\s*source = \w+.\w+,\s*(siteId = \d+,\s*)?(?:site)?Domain = "(?P<domain>[\w\.\-/]+)""#)
        .case_insensitive(true)
        .build()
        .unwrap())
]
});

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KotatsuMangaBackup {
    pub id: i64,
    pub title: String,
    pub alt_tile: Option<String>,
    pub url: String,
    pub public_url: String,
    pub rating: f32,
    pub nsfw: bool,
    pub cover_url: String,
    pub large_cover_url: Option<String>,
    pub state: String,
    pub author: String,
    pub source: String,
    // neko backups do not provide the relevant links, only the names
    // as such, this is just here to appease the expected json format
    pub tags: [String; 0],
}

#[derive(Debug, Serialize, Deserialize)]
pub struct KotatsuHistoryBackup {
    pub manga_id: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub chapter_id: i64,
    pub page: i32,
    pub scroll: f32,
    pub percent: f32,
    pub manga: KotatsuMangaBackup,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct KotatsuCategoryBackup {
    pub category_id: i64,
    pub created_at: i64,
    pub sort_key: i32,
    pub title: String,
    pub order: Option<KotatsuListSortOrder>,
    pub track: Option<bool>,
    pub show_in_lib: Option<bool>,
    pub deleted_at: i64,
}

#[allow(non_camel_case_types)]
#[derive(Debug, Serialize, Deserialize)]
pub enum KotatsuListSortOrder {
    NEWEST,
    OLDEST,
    PROGRESS,
    UNREAD,
    ALPHABETIC,
    ALPHABETIC_REVERSE,
    RATING,
    RELEVANCE,
    NEW_CHAPTERS,
    LAST_READ,
    LONG_AGO_READ,
    UPDATED,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct KotatsuFavouriteBackup {
    pub manga_id: i64,
    pub category_id: i64,
    pub sort_key: i32,
    pub created_at: i64,
    pub deleted_at: i64,
    pub manga: KotatsuMangaBackup,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct KotatsuBookmarkBackup {
    pub manga: KotatsuMangaBackup,
    pub tags: [String; 0],
    pub bookmarks: Vec<KotatsuBookmarkEntry>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct KotatsuBookmarkEntry {
    pub manga_id: i64,
    pub page_id: i64,
    pub chapter_id: i64,
    pub page: i32,
    pub scroll: i32,
    pub image_url: String,
    pub created_at: i64,
    pub percent: f32,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum KotatsuParserContentType {
    Manga,
    Hentai,
    Comics,
    Other,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct KotatsuParser {
    pub name: String,
    pub title: String,
    pub locale: Option<String>,
    pub content_type: KotatsuParserContentType,
    pub domains: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct KotatsuIndexEntry {
    pub app_id: String,
    pub app_version: u64,
    pub created_at: u128,
}

impl KotatsuIndexEntry {
    pub fn generate() -> Self {
        Self {
            app_id: String::from("com.github.phantomshift.nekotatsu"),
            app_version: 0,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        }
    }
}

pub fn get_kotatsu_id(source_name: &str, url: &str) -> i64 {
    let mut id: i64 = 1125899906842597;
    source_name
        .chars()
        .for_each(|c| id = (31i64.overflowing_mul(id)).0.overflowing_add(c as i64).0);
    url.chars()
        .for_each(|c| id = (31i64.overflowing_mul(id)).0.overflowing_add(c as i64).0);
    return id;
}

fn get_parser_definitions(
    archive: ZipArchive<Cursor<Vec<u8>>>,
) -> std::io::Result<Vec<(String, String)>> {
    let mut files = Vec::new();

    let root = archive
        .file_names()
        .nth(0)
        .ok_or(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Archive is empty",
        ))?
        .chars()
        .take_while(|&c| c != '/')
        .collect::<String>();

    for path in archive.file_names() {
        if path.contains(&format!(
            "{root}/src/main/kotlin/org/koitharu/kotatsu/parsers/site/"
        )) && path.ends_with(".kt")
        {
            let mut clone = archive.clone();
            let mut file = clone.by_name(path)?;
            let mut s = String::new();
            file.read_to_string(&mut s)?;
            files.push((s, path.to_string()));
        }
    }

    Ok(files)
}

pub fn update_parsers(new: &File, mut save_to: &File) -> std::io::Result<()> {
    let reader = BufReader::new(new);
    let bytes = Cursor::new(
        reader
            .bytes()
            .collect::<Result<Vec<u8>, std::io::Error>>()?,
    );
    let reader = zip::read::ZipArchive::new(bytes)?;
    let files = get_parser_definitions(reader)?;
    let mut parsers = Vec::new();
    for (contents, path) in files.iter() {
        // (Known) parsers I will likely need to make custom code for: ExHentai and NineManga
        let captures = PARSER_CAPTURE.captures_iter(&contents).collect::<Vec<_>>();
        if captures.len() == 0 {
            continue;
        }

        let domains = DOMAIN_CAPTURE_METHODS
            .iter()
            .find_map(|method| method.capture_domains(&contents))
            .unwrap_or(Vec::new());

        if domains.len() == 0 {
            warn!("Kotatsu parser was detected but domains could not be found automatically. File path: '{path}'")
        }

        for c in captures {
            let parser = KotatsuParser {
                name: c["name"].to_string(),
                title: c["title"].to_string(),
                locale: c
                    .name("locale")
                    .map_or(None, |locale| Some(locale.as_str().to_string())),
                content_type: match c.name("type").map(|t| t.as_str()) {
                    Some("ContentType.MANGA") => KotatsuParserContentType::Manga,
                    Some("ContentType.HENTAI") => KotatsuParserContentType::Hentai,
                    Some("ContentType.COMICS") => KotatsuParserContentType::Comics,
                    Some("ContentType.OTHER") => KotatsuParserContentType::Other,
                    Some(_) | None => KotatsuParserContentType::Manga,
                },
                domains: domains.clone(),
            };
            parsers.push(parser);
        }
    }

    save_to.write(&mut serde_json::to_vec(&parsers)?)?;

    Ok(())
}
