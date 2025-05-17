use extensions::SourceInfo;
use prost::Message;
use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    fs::File,
    io::{self, Read, Write},
};

pub mod config;
pub mod extensions;
pub mod nekotatsu {
    pub mod neko {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/neko.backup.rs"));
    }
}
pub mod kotatsu;
use kotatsu::*;

const CATEGORY_DEFAULT: i64 = 2;
const CATEGORY_OFFSET: i64 = CATEGORY_DEFAULT + 1;

#[allow(unused_variables)]
pub trait Logger {
    fn log_info(&mut self, message: &str) -> () {}
    fn log_verbose(&mut self, message: &str) -> () {
        self.log_info(message);
    }
    fn log_very_verbose(&mut self, message: &str) -> () {
        self.log_verbose(message);
    }

    fn capture_output(&mut self) -> String {
        String::new()
    }
}

#[derive(Debug)]
pub struct MangaConverter {
    sources: HashMap<i64, String>,
    parsers: Vec<KotatsuParser>,
    pub extensions: extensions::ExtensionList,

    soft_match: bool,
}

pub struct MangaConversionResult {
    pub categories: Vec<KotatsuCategoryBackup>,
    pub favourites: Vec<KotatsuFavouriteBackup>,
    pub history: Vec<KotatsuHistoryBackup>,
    pub bookmarks: Vec<KotatsuBookmarkBackup>,
    pub errored_sources: HashMap<String, String>,
    pub errored_sources_count: HashMap<String, usize>,
    pub unknown_sources: HashSet<String>,
    pub total_manga: usize,
    pub errored_manga: usize,
    pub ignored_manga: usize,
}

impl MangaConverter {
    pub fn new() -> Self {
        Self {
            sources: HashMap::new(),
            parsers: Vec::new(),
            extensions: extensions::ExtensionList::default(),
            soft_match: false,
        }
    }

    pub fn with_sources(self, sources: HashMap<i64, String>) -> Self {
        Self { sources, ..self }
    }

    pub fn with_parsers(self, parsers: Vec<KotatsuParser>) -> Self {
        Self { parsers, ..self }
    }

    pub fn with_extensions(self, extensions: extensions::ExtensionList) -> Self {
        Self { extensions, ..self }
    }

    pub fn with_soft_match(self, enabled: bool) -> Self {
        Self {
            soft_match: enabled,
            ..self
        }
    }

    pub fn try_from_files(mut parsers: File, extensions: File) -> std::io::Result<Self> {
        let mut parser_list = String::new();
        parsers.read_to_string(&mut parser_list)?;
        let parsers: Vec<KotatsuParser> = serde_json::from_str(&parser_list)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let extensions = extensions::ExtensionList::try_from_file(extensions)?;
        let sources = HashMap::new();

        Ok(Self {
            sources,
            parsers,
            extensions,
            soft_match: false,
        })
    }

    pub fn get_source_name(&mut self, manga: &nekotatsu::neko::BackupManga) -> String {
        match manga.source {
            // Hardcoded
            2499283573021220255 => "MANGADEX".to_owned(),
            1998944621602463790 => "MANGAPLUSPARSER_EN".to_owned(),

            id => {
                self.sources
                    .entry(id)
                    .or_insert_with(|| {
                        if let Some(source) = self.extensions.get_source(id) {
                            let urls = vec![
                                source
                                    .baseUrl
                                    .trim_start_matches("http://")
                                    .trim_start_matches("https://")
                                    .to_string(),
                                source
                                    .baseUrl
                                    .trim_start_matches("http://")
                                    .trim_start_matches("https://")
                                    .trim_start_matches("www.")
                                    .to_string(),
                            ];

                            self.parsers
                                .iter()
                                .find(|p| {
                                    p.name.to_lowercase() == source.name
                                        || p.domains.iter().any(|d| urls.iter().any(|url| d == url))
                                })
                                .or(self
                                    .soft_match
                                    .then_some({
                                        // Boldly assuming that there's only one relevant top-level domain
                                        let url = source
                                            .baseUrl
                                            .trim_start_matches("http://")
                                            .trim_start_matches("https://");
                                        match url.rsplit_once(".") {
                                            Some((name, _tld)) => self.parsers.iter().find(|p| {
                                                p.domains.iter().any(|d| d.contains(name))
                                            }),
                                            None => None,
                                        }
                                    })
                                    .flatten())
                                .map_or(String::from("UNKNOWN"), |p| p.name.clone())
                        } else {
                            String::from("UNKNOWN")
                        }
                    })
                    .to_string()
            }
        }
    }

    fn manga_to_kotatsu(
        &mut self,
        manga: &nekotatsu::neko::BackupManga,
    ) -> Option<KotatsuMangaBackup> {
        let source_info = self.extensions.get_source(manga.source)?;
        let domain = source_info.baseUrl;
        let source_name = self.get_source_name(manga);
        let relative_url = kotatsu::correct_relative_url(&source_name, &manga.url);
        let manga_identifier = kotatsu::correct_identifier(&source_name, &relative_url);

        Some(KotatsuMangaBackup {
            id: get_kotatsu_id(&source_name, &manga_identifier),
            title: manga.title.clone(),
            alt_tile: None,
            url: relative_url.clone(),
            public_url: kotatsu::correct_public_url(&source_name, &domain, &relative_url),
            rating: -1.0,
            nsfw: false,
            cover_url: manga.thumbnail_url.clone(),
            large_cover_url: Some(manga.thumbnail_url.clone()),
            author: manga.author.clone(),
            state: String::from(match manga.status {
                1 => "ONGOING",
                2 | 4 => "FINISHED",
                5 => "ABANDONED",
                6 => "PAUSED",
                _ => "",
            }),
            source: source_name.clone(),
            tags: [],
        })
    }

    pub fn convert_backup(
        mut self,
        backup: nekotatsu::neko::Backup,
        favorites_name: &str,
        logger: &mut dyn Logger,
        source_filter: &mut dyn FnMut(&SourceInfo) -> bool,
    ) -> MangaConversionResult {
        let mut result_categories = Vec::with_capacity(backup.backup_categories.len() + 1);
        let mut result_favourites = Vec::with_capacity(backup.backup_manga.len());
        let mut result_history = Vec::with_capacity(backup.backup_manga.len());
        let mut result_bookmarks = Vec::new();
        let mut errored_sources = HashMap::new();
        let mut errored_sources_count: HashMap<String, usize> = HashMap::new();
        let mut unknown_sources = HashSet::new();
        let mut errored_manga = 0;
        let mut ignored_manga = 0;

        result_categories.push(KotatsuCategoryBackup {
            category_id: CATEGORY_DEFAULT,
            created_at: 0,
            sort_key: 0,
            title: favorites_name.into(),
            order: Some("NAME".into()),
            track: Some(true),
            show_in_lib: Some(true),
            deleted_at: 0,
        });
        result_categories.extend(backup.backup_categories.iter().enumerate().map(
            |(id, category)| KotatsuCategoryBackup {
                category_id: id as i64 + CATEGORY_OFFSET,
                created_at: 0,
                sort_key: category.order,
                title: category.name.clone(),
                order: None,
                // TODO: convert flags
                // see https://github.com/mihonapp/mihon/blob/main/domain/src/main/java/tachiyomi/domain/library/model/LibrarySortMode.kt
                track: None,
                show_in_lib: Some(true),
                deleted_at: 0,
            },
        ));

        for manga in backup.backup_manga.iter() {
            if manga.source == 0 {
                logger.log_verbose(&format!(
                    "[WARNING] Unable to convert '{}', local manga currently unsupported",
                    manga.title
                ));
                errored_manga += 1;
                continue;
            }

            let source = self
                .extensions
                .get_source(manga.source)
                .unwrap_or(SourceInfo {
                    id: manga.source.to_string(),
                    ..Default::default()
                });

            if !source_filter(&source) {
                ignored_manga += 1;
                continue;
            }

            if source.name == SourceInfo::default().name {
                let message = format!(
                    "[WARNING] Unable to convert '{}', unknown Tachiyomi source (ID {})",
                    manga.title, manga.source
                );
                if unknown_sources.contains(&manga.source.to_string()) {
                    logger.log_very_verbose(&message);
                } else {
                    logger.log_verbose(&message);
                    unknown_sources.insert(manga.source.to_string());
                }

                errored_sources.insert(source.name.clone(), source.baseUrl);
                errored_sources_count
                    .entry(source.name.clone())
                    .and_modify(|e| *e += 1)
                    .or_insert(1);
                errored_manga += 1;
                continue;
            }

            let kotatsu_manga = self
                .manga_to_kotatsu(&manga)
                .expect("unknown Tachiyomi source not filtered");

            if kotatsu_manga.source == "UNKNOWN" {
                let message = format!(
                    "[WARNING] Unable to convert '{}' from source {} ({}), Kotatsu parser not found",
                    manga.title, source.name, source.baseUrl
                );
                if errored_sources.contains_key(&source.name) {
                    logger.log_very_verbose(&message)
                } else {
                    logger.log_verbose(&message);
                    errored_sources.insert(source.name.clone(), source.baseUrl);
                }
                errored_sources_count
                    .entry(source.name.clone())
                    .and_modify(|e| *e += 1)
                    .or_insert(1);
                errored_manga += 1;
                continue;
            }

            result_favourites.extend(
                manga
                    .categories
                    .iter()
                    .map(|id| *id as i64 + CATEGORY_OFFSET)
                    .chain(std::iter::once(CATEGORY_DEFAULT))
                    .map(|id| KotatsuFavouriteBackup {
                        manga_id: kotatsu_manga.id.clone(),
                        category_id: id,
                        sort_key: 0,
                        created_at: 0,
                        deleted_at: 0,
                        manga: kotatsu_manga.clone(),
                    }),
            );

            let latest_chapter =
                manga
                    .chapters
                    .iter()
                    .fold(None, |current, checking| match current {
                        None if checking.read => Some(checking),
                        Some(current)
                            if checking.read
                                && checking.chapter_number > current.chapter_number =>
                        {
                            Some(checking)
                        }
                        _ => current,
                    });
            let bookmarks: Vec<KotatsuBookmarkEntry> = manga
                .chapters
                .iter()
                .filter_map(|chapter| {
                    chapter.bookmark.then(|| KotatsuBookmarkEntry {
                        manga_id: kotatsu_manga.id,
                        page_id: 0,
                        chapter_id: get_kotatsu_id(
                            &kotatsu_manga.source,
                            &correct_identifier(&kotatsu_manga.source, &chapter.url),
                        ),
                        page: chapter.last_page_read,
                        scroll: 0,
                        image_url: kotatsu_manga.cover_url.clone(),
                        created_at: 0,
                        percent: match chapter.last_page_read + chapter.pages_left {
                            0 => 0.0,
                            total_pages => chapter.last_page_read as f32 / total_pages as f32,
                        },
                    })
                })
                .collect();
            if bookmarks.len() > 0 {
                result_bookmarks.push(KotatsuBookmarkBackup {
                    manga: kotatsu_manga.clone(),
                    tags: [],
                    bookmarks,
                })
            }
            let newest_cached_chapter = manga
                .chapters
                .iter()
                .max_by(|a, b| a.chapter_number.total_cmp(&b.chapter_number));
            let last_read = manga
                .history
                .iter()
                .max_by(|l, r| l.last_read.cmp(&r.last_read))
                .map(|entry| entry.last_read)
                .unwrap_or(manga.last_update);
            let kotatsu_history = KotatsuHistoryBackup {
                manga_id: kotatsu_manga.id.clone(),
                created_at: manga.date_added,
                updated_at: last_read,
                chapter_id: if let Some(latest) = latest_chapter {
                    get_kotatsu_id(
                        &kotatsu_manga.source,
                        &correct_identifier(&kotatsu_manga.source, &latest.url),
                    )
                } else {
                    0
                },
                page: latest_chapter
                    .map(|latest| latest.last_page_read)
                    .unwrap_or(0),
                scroll: 0.0,
                percent: match (latest_chapter, newest_cached_chapter) {
                    (Some(latest), Some(newest)) if latest.chapter_number > 0.0 => {
                        (latest.chapter_number - 1.0) / newest.chapter_number
                    }
                    _ => 0.0,
                },
                manga: kotatsu_manga,
            };

            result_history.push(kotatsu_history)
        }

        MangaConversionResult {
            categories: result_categories,
            favourites: result_favourites,
            history: result_history,
            bookmarks: result_bookmarks,
            errored_manga,
            errored_sources_count,
            unknown_sources,
            total_manga: backup.backup_manga.len(),
            errored_sources,
            ignored_manga,
        }
    }
}

impl Logger for std::io::Stdout {
    fn log_info(&mut self, message: &str) -> () {
        let _ = self.write(message.as_bytes());
        let _ = self.write(b"\n");
    }
}

impl Logger for Vec<String> {
    fn log_info(&mut self, message: &str) -> () {
        self.push(message.to_string());
    }

    fn capture_output(&mut self) -> String {
        self.join("\n")
    }
}

fn decode_gzip_backup(file: File) -> std::io::Result<Vec<u8>> {
    let mut reader = std::io::BufReader::new(file);
    let mut decoder = flate2::read::GzDecoder::new(&mut reader);
    let mut buf = Vec::new();
    decoder.read_to_end(&mut buf)?;

    return Ok(buf);
}

pub fn decode_neko_backup(file: File) -> std::io::Result<nekotatsu::neko::Backup> {
    let neko_read = decode_gzip_backup(file)
        .or_else(|e| {
            Err(match e.kind() {
                io::ErrorKind::Interrupted | io::ErrorKind::InvalidInput => io::Error::new(std::io::ErrorKind::InvalidInput,
                    format!("Error occurred when parsing input archive, is it an actual neko backup? Original error: {e}")
                ),
                _ => e
            })
        })?;

    Ok(nekotatsu::neko::Backup::decode(&mut neko_read.as_slice())?)
}
