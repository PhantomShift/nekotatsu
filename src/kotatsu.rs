use std::io::{Cursor, Read};

use serde::{Serialize, Deserialize};
use zip::ZipArchive;
use regex::Regex;
use lazy_static::lazy_static;

lazy_static! {
    static ref PARSER_CAPTURE: Regex = Regex::new(r#"@MangaSourceParser\(.(?P<name>\w*)., .(?P<title>[\w\s\(\)]+).(, .(?P<locale>\w*).(, (?P<type>[\w\.]+))?)?"#).unwrap();
    // static ref DOMAIN_CAPTURE_CUSTOM: Regex = Regex::new(r#"\w+Parser\(context, MangaSource\.\w+, .(?P<domain>[\w\.\-]+)."#).unwrap();
    static ref DOMAIN_CAPTURE_CUSTOM: Regex = Regex::new(r#"\(\s*context,\s*MangaSource\.\w+,\s*.(?P<domain>[\w\.\-]+)."#).unwrap();
    // static ref DOMAIN_CAPTURE: Regex = Regex::new(r#"ConfigKey\.Domain\((?P<domains>.+)\)"#).unwrap();
    static ref DOMAIN_CAPTURE: Regex = regex::RegexBuilder::new(r#"ConfigKey\.Domain\((?P<domains>.+?)\)"#)
        .dot_matches_new_line(true)
        .build()
        .unwrap();
}

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
    pub tags: [String;0],
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
    pub order: Option<String>,
    pub track: Option<bool>,
    pub show_in_lib: Option<bool>,
    pub deleted_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct KotatsuFavouriteBackup {
    pub manga_id: i64,
    pub category_id: i64,
    pub sort_key: i32,
    pub created_at: i64,
    pub deleted_at: i64,
    pub manga: KotatsuMangaBackup
}

#[derive(Debug, Serialize, Deserialize)]
pub struct KotatsuBookmarkBackup {
    pub manga: KotatsuMangaBackup,
    pub tags: [String;0],
    pub bookmarks: Vec<KotatsuBookmarkEntry>
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
    pub percent: f32
}

#[derive(Debug, Serialize, Deserialize)]
pub enum KotatsuParserContentType {
    Manga,
    Hentai,
    Comics,
    Other
}
#[derive(Debug, Serialize, Deserialize)]
pub struct KotatsuParser {
    pub name: String,
    pub title: String,
    pub locale: Option<String>,
    pub content_type: KotatsuParserContentType,
    pub domains: Vec<String>
}

pub fn get_kotatsu_id(source_name: &str, url: &str) -> i64 {
    let mut id: i64 = 1125899906842597;
    source_name.chars().for_each(|c| id = (31i64.overflowing_mul(id)).0.overflowing_add(c as i64).0);
    url.chars().for_each(|c| id = (31i64.overflowing_mul(id)).0.overflowing_add(c as i64).0);
    return id
}

/// Correct identifiers for known sources; leaves alone if not implemented
pub fn correct_identifier(source_name: &str, identifier: &str) -> String {
    match source_name {
        "MANGADEX" => {
            identifier.replace("/title/", "")
                .replace("/chapter/", "")
        },
        _ => identifier.to_string()
    }
}
/// Correct urls for known sources; leaves alone if not implemented
pub fn correct_url(source_name: &str, url: &str) -> String {
    match source_name {
        "MANGADEX" => url.replace("/manga/", "/title/"),
        _ => url.to_string()
    }
}

fn get_parser_definitions(archive: ZipArchive<Cursor<Vec<u8>>>) -> std::io::Result<Vec<String>> {
    let mut files = Vec::new();

    let root = archive.file_names().nth(0)
        .ok_or(std::io::Error::new(std::io::ErrorKind::InvalidData, "Archive is empty"))?
        .chars().take_while(|&c| c != '/')
        .collect::<String>();

    for path in archive.file_names() {
        if path.contains(&format!("{root}/src/main/kotlin/org/koitharu/kotatsu/parsers/site/"))
        && path.ends_with(".kt") {
            let mut clone = archive.clone();
            let mut file = clone.by_name(path)?;
            let mut s = String::new();
            file.read_to_string(&mut s)?;
            files.push(s);
        }
    }

    Ok(files)
}

pub fn update_parsers(path: &str) -> std::io::Result<()> {
    let bytes = Cursor::new(std::fs::File::open(path)?.bytes().collect::<Result<Vec<u8>, std::io::Error>>()?);
    let reader = zip::read::ZipArchive::new(bytes)?;
    let files = get_parser_definitions(reader)?;
    let mut parsers = Vec::new();
    for s in files.iter() {
        let domains = {
            // (Known) parsers I will likely need to make custom code for: ExHentai and NineManga
            if let Some(c) = DOMAIN_CAPTURE.captures(s) {
                let list = &c["domains"];
                list.replace(['\n', '\t', ' '], "")
                    .split(",")
                    .map(|d| d.replace('"', ""))
                    .filter(|s| !s.is_empty())
                    .collect()
            } else if let Some(c) = DOMAIN_CAPTURE_CUSTOM.captures(s) {
                vec![c["domain"].to_string()]
            } else {
                Vec::new()
            }
        };

        for c in PARSER_CAPTURE.captures_iter(s) {
            let parser = KotatsuParser {
                name: c["name"].to_string(),
                title: c["title"].to_string(),
                locale: c.name("locale").map_or(None, |locale| {
                    Some(locale.as_str().to_string())
                }),
                content_type: match c.name("type").map(|t| t.as_str()) {
                    Some("ContentType.MANGA") => KotatsuParserContentType::Manga,
                    Some("ContentType.HENTAI") => KotatsuParserContentType::Hentai,
                    Some("ContentType.COMICS") => KotatsuParserContentType::Comics,
                    Some("ContentType.OTHER") => KotatsuParserContentType::Other,
                    Some(_) | None => KotatsuParserContentType::Manga,
                },
                domains: domains.clone()
            };
            parsers.push(parser);
        }
    }
    // let to_store = serde_json::to_string_pretty(&parsers)?;
    let to_store = serde_json::to_string(&parsers)?;
    std::fs::write("kotatsu_parsers.json", &to_store)?;

    Ok(())
}