use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{self, Read, Write},
    path::PathBuf,
};

use clap::{Parser, Subcommand};
use config::{SourceFilterEntry, SourceFilterList};
use directories::ProjectDirs;
use extensions::SourceInfo;
use flate2::{write::GzEncoder, Compression};
use lazy_static::lazy_static;
use prost::Message;

pub mod config;
pub mod extensions;
pub mod nekotatsu {
    pub mod neko {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/neko.backup.rs"));
    }
}
pub mod kotatsu;
use kotatsu::*;

lazy_static! {
    static ref PROJECT_DIR: ProjectDirs =
        ProjectDirs::from("", "", "Nekotatsu").expect("Invalid application directories generated");
    static ref TACHI_SOURCE_PATH: PathBuf =
        PathBuf::from(PROJECT_DIR.data_dir()).join("tachi_sources.json");
    static ref KOTATSU_PARSE_PATH: PathBuf =
        PathBuf::from(PROJECT_DIR.data_dir()).join("kotatsu_parsers.json");
}

const CATEGORY_DEFAULT: i64 = 2;
const CATEGORY_OFFSET: i64 = CATEGORY_DEFAULT + 1;

/// Simple CLI tool that converts Neko backups into Kotatsu backups
#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Convert a Neko/Tachiyomi backup into one that Kotatsu can read
    Convert {
        /// Path to Neko/Tachi backup
        input: String,

        /// Optional output name
        #[arg(short, long)]
        output: Option<String>,

        /// Category name for favorited manga.
        #[arg(alias("fn"), long, default_value_t = String::from("Library"))]
        favorites_name: String,

        /// Display some additional information. Overrides verbose option.
        #[arg(short, long)]
        verbose: bool,

        /// Display all debug information
        #[arg(short('V'), long)]
        very_verbose: bool,

        /// Convert to Neko instead
        #[arg(short, long)]
        reverse: bool,

        /// Strip top-level domains when comparing Tachiyomi/Mihon sources to Kotatsu parsers
        #[arg(short, long)]
        soft_match: bool,

        /// Convert without asking about overwriting existing files
        #[arg(short, long)]
        force: bool,

        #[arg(short, long)]
        config_file: Option<PathBuf>,

        #[arg(long, hide = true, default_value_t = true)]
        print_output: bool,
    },

    /// Downloads latest Tachiyomi source information and
    /// updates Kotatsu parser list. The resulting files are saved in the app's data directory
    /// (`~/.local/share/nekotatsu` on Linux and `%APPDATA%\Nekotatsu\data` on Windows)
    /// as `tachi_sources.json` and `kotatsu_parsers.json`.
    Update {
        /// Download URL for Kotatsu parsers repo.
        #[arg(short, long, default_value_t = String::from("https://github.com/KotatsuApp/kotatsu-parsers/archive/refs/heads/master.zip"))]
        kotatsu_link: String,

        /// Download URL for Tachiyomi extension json list (minified)
        #[arg(short, long, default_value_t = String::from("https://raw.githubusercontent.com/keiyoushi/extensions/repo/index.min.json"))]
        tachi_link: String,

        /// Force download of files even if they already exist
        #[arg(short, long)]
        force_download: bool,
    },

    /// Output backup info
    #[command(hide(true))]
    Debug { input: String },

    /// Deletes any files downloaded by nekotatsu (the data directory);
    /// Effectively the same as running `rm -rf ~/.local/share/nekotatsu` on Linux and `rmdir /s /q %APPDATA%\Nekotatsu` on Windows.
    Clear,
    /// Alias for `clear`
    Delete,
}

pub enum CommandVerbosity {
    None,
    Verbose,
    VeryVerbose,
}

impl CommandVerbosity {
    fn should_display(&self, has_occurred: bool) -> bool {
        match *self {
            CommandVerbosity::None => false,
            CommandVerbosity::Verbose => !has_occurred,
            CommandVerbosity::VeryVerbose => true,
        }
    }
}

pub enum CommandResult {
    None,
    Success(String, String),
}

pub enum Buffer {
    Stdout(std::io::Stdout),
    Vector(Vec<u8>),
}

impl Buffer {
    fn write_fmt(&mut self, args: std::fmt::Arguments) -> Result<(), std::io::Error> {
        match self {
            Self::Stdout(stdout) => stdout.write_fmt(args),
            Self::Vector(ref mut v) => v.write_fmt(args),
        }
    }
}

pub struct MangaConverter {
    sources: HashMap<i64, String>,
    parsers: Vec<KotatsuParser>,
    pub extensions: extensions::ExtensionList,

    soft_match: bool,
}

impl MangaConverter {
    fn try_from_files(mut parsers: File, extensions: File) -> std::io::Result<Self> {
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

    fn with_soft_match(self, soft_match: bool) -> Self {
        Self { soft_match, ..self }
    }

    fn get_source_name(&mut self, manga: &nekotatsu::neko::BackupManga) -> String {
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
        let relative_url = kotatsu::correct_url(&source_name, &manga.url);
        let manga_identifier = kotatsu::correct_identifier(&source_name, &relative_url);

        Some(KotatsuMangaBackup {
            id: get_kotatsu_id(&source_name, &manga_identifier),
            title: manga.title.clone(),
            alt_tile: None,
            url: relative_url.clone(),
            public_url: format!("{domain}{relative_url}"),
            rating: -1.0,
            nsfw: false,
            cover_url: format!("{}.256.jpg", manga.thumbnail_url),
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
}

fn decode_gzip_backup(path: &str) -> std::io::Result<Vec<u8>> {
    let bytes = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(bytes);
    let mut decoder = flate2::read::GzDecoder::new(&mut reader);
    let mut buf = Vec::new();
    decoder.read_to_end(&mut buf)?;

    return Ok(buf);
}

fn neko_to_kotatsu(
    input_path: String,
    output_path: PathBuf,
    verbosity: CommandVerbosity,
    favorites_name: String,
    soft_match: bool,
    print_output: bool,
    config: config::ConfigFile,
) -> std::io::Result<CommandResult> {
    let mut buffer = if print_output {
        Buffer::Stdout(std::io::stdout())
    } else {
        Buffer::Vector(Vec::new())
    };

    let mut converter = MangaConverter::try_from_files(
        std::fs::File::open(&KOTATSU_PARSE_PATH.as_path())?,
        std::fs::File::open(&TACHI_SOURCE_PATH.as_path())?,
    )?
    .with_soft_match(soft_match);

    let neko_read = decode_gzip_backup(&input_path)
        .or_else(|e| {
            Err(match e.kind() {
                io::ErrorKind::Interrupted | io::ErrorKind::InvalidInput => io::Error::new(std::io::ErrorKind::InvalidInput,
                    format!("Error occurred when parsing input archive, is it an actual neko backup? Original error: {e}")
                ),
                _ => e
            })
        })?;

    let backup = nekotatsu::neko::Backup::decode(&mut neko_read.as_slice())?;
    let mut result_categories = Vec::with_capacity(backup.backup_categories.len() + 1);
    let mut result_favourites = Vec::with_capacity(backup.backup_manga.len());
    let mut result_history = Vec::with_capacity(backup.backup_manga.len());
    let mut result_bookmarks = Vec::new();
    let mut total_manga = 0;
    let mut errored_manga = 0;
    let mut errored_sources = HashMap::new();
    let mut errored_sources_count: HashMap<String, usize> = HashMap::new();
    let mut unknown_sources = HashSet::new();

    // Possible todo: convert to hash set
    // Likely not necessary though unless the list is very large
    let source_whitelist = config.whitelist.unwrap_or(Vec::new());
    let source_blacklist = config.blacklist.unwrap_or(Vec::new());
    let mut ignored_manga = 0;

    result_categories.push(KotatsuCategoryBackup {
        category_id: CATEGORY_DEFAULT,
        created_at: 0,
        sort_key: 0,
        title: favorites_name,
        order: Some(String::from("NAME")),
        track: Some(true),
        show_in_lib: Some(true),
        deleted_at: 0,
    });
    for (id, category) in backup.backup_categories.iter().enumerate() {
        result_categories.push(KotatsuCategoryBackup {
            // kotatsu appears to not allow index 0 for category id
            category_id: id as i64 + CATEGORY_OFFSET,
            created_at: 0,
            sort_key: category.order,
            title: category.name.clone(),
            order: None,
            track: None,
            show_in_lib: Some(true),
            deleted_at: 0,
        });
    }

    for manga in backup.backup_manga.iter() {
        total_manga += 1;

        // ignore locally imported manga
        if manga.source == 0 {
            if matches!(
                verbosity,
                CommandVerbosity::Verbose | CommandVerbosity::VeryVerbose
            ) {
                buffer.write_fmt(format_args!(
                    "[WARNING] Unable to convert '{}', local manga currently unsupported\n",
                    manga.title
                ))?;
            }
            errored_manga += 1;
            continue;
        }

        let source = converter
            .extensions
            .get_source(manga.source)
            .unwrap_or(SourceInfo {
                id: manga.source.to_string(),
                ..Default::default()
            });
        if source.name == SourceInfo::default().name {
            if source_whitelist.len() > 0
                && !source_whitelist.contains(&SourceFilterEntry::Id(manga.source))
            {
                ignored_manga += 1;
                continue;
            }
            if verbosity.should_display(unknown_sources.contains(&manga.source.to_string())) {
                buffer.write_fmt(format_args!(
                    "[WARNING] Unable to convert '{}', unknown Tachiyomi source (ID {})\n",
                    manga.title, manga.source
                ))?;
            }
            errored_sources.insert(source.name.clone(), source.baseUrl);
            errored_sources_count
                .entry(source.name.clone())
                .and_modify(|e| *e += 1)
                .or_insert(1);
            unknown_sources.insert(manga.source.to_string());
            errored_manga += 1;
            continue;
        }

        if !source_whitelist.check_source(true, &source)
            || source_blacklist.check_source(false, &source)
        {
            ignored_manga += 1;
            continue;
        }

        let kotatsu_manga = converter
            .manga_to_kotatsu(&manga)
            .expect("unknown sources should be filtered");

        if kotatsu_manga.source == "UNKNOWN" {
            if verbosity.should_display(errored_sources.get(&source.name).is_some()) {
                buffer.write_fmt(format_args!("[WARNING] Unable to convert '{}' from source {} ({}), Kotatsu parser not found\n", manga.title, source.name, source.baseUrl))?;
            }
            errored_sources.insert(source.name.clone(), source.baseUrl);
            errored_sources_count
                .entry(source.name.clone())
                .and_modify(|e| *e += 1)
                .or_insert(1);
            errored_manga += 1;
            continue;
        }

        let make_fav_backup = |id: i64| KotatsuFavouriteBackup {
            manga_id: kotatsu_manga.id.clone(),
            category_id: id,
            sort_key: 0,
            created_at: 0,
            deleted_at: 0,
            manga: kotatsu_manga.clone(),
        };
        for category_id in manga.categories.iter() {
            result_favourites.push(make_fav_backup(*category_id as i64 + CATEGORY_OFFSET))
        }
        result_favourites.push(make_fav_backup(CATEGORY_DEFAULT));

        let latest_chapter = manga
            .chapters
            .iter()
            .fold(None, |current, checking| match current {
                None if checking.read => Some(checking),
                Some(current)
                    if checking.read && checking.chapter_number > current.chapter_number =>
                {
                    Some(checking)
                }
                _ => current,
            });
        let bookmarks = manga
            .chapters
            .iter()
            .filter_map(|chapter| {
                if !chapter.bookmark {
                    return None;
                }
                Some(KotatsuBookmarkEntry {
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
                    percent: if chapter.last_page_read + chapter.pages_left == 0 {
                        0.0
                    } else {
                        chapter.last_page_read as f32
                            / (chapter.last_page_read as f32 + chapter.pages_left as f32)
                    },
                })
            })
            .collect::<Vec<_>>();
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
            page: if let Some(latest) = latest_chapter {
                latest.last_page_read
            } else {
                0
            },
            scroll: 0.0,
            percent: match (latest_chapter, newest_cached_chapter) {
                (Some(latest), Some(newest)) if latest.chapter_number > 0.0 => {
                    (latest.chapter_number - 1.0) / newest.chapter_number
                }
                _ => 0.0,
            },
            manga: kotatsu_manga,
        };
        result_history.push(kotatsu_history);
    }

    let to_make = std::fs::File::create(output_path.clone())?;
    let options = zip::write::FileOptions::default();
    let mut writer = zip::ZipWriter::new(to_make);
    for (name, entry) in [
        ("history", serde_json::to_string_pretty(&result_history)),
        (
            "categories",
            serde_json::to_string_pretty(&result_categories),
        ),
        (
            "favourites",
            serde_json::to_string_pretty(&result_favourites),
        ),
        ("bookmarks", serde_json::to_string_pretty(&result_bookmarks)),
        (
            "index",
            serde_json::to_string_pretty(&[kotatsu::KotatsuIndexEntry::generate()]),
        ),
    ] {
        match entry {
            Ok(json) => {
                if json.trim_end() != "[]" {
                    writer.start_file(name, options)?;
                    writer.write_all(json.as_bytes())?;
                }
            }
            #[allow(unreachable_patterns)]
            Ok(_) => {
                buffer.write_fmt(format_args!(
                    "{name} is empty, ommitted from converted backup\n"
                ))?;
            }
            Err(e) => {
                buffer.write_fmt(format_args!(
                    "Warning: Error occured processing {name}, ommitted from converted backup\n"
                ))?;
                buffer.write_fmt(format_args!("Original error: {e}\n"))?;
            }
        }
    }
    writer.finish()?;

    if errored_manga > 0 {
        buffer.write_fmt(format_args!("{errored_manga} of {total_manga} manga and {} sources failed to convert ({} unknown).\n", errored_sources.len(), unknown_sources.len()))?;
        if matches!(verbosity, CommandVerbosity::None) {
            buffer.write_fmt(format_args!(
                "Try running again with verbose (-v) on for details\n"
            ))?;
        } else {
            match verbosity {
                CommandVerbosity::None => (),
                CommandVerbosity::Verbose => {
                    buffer.write_fmt(format_args!(
                        "Sources that errorred: {}\n",
                        errored_sources
                            .keys()
                            .into_iter()
                            .fold(String::new(), |mut a, s| {
                                a.push_str(s);
                                a.push_str(", ");
                                a
                            })
                            .trim_end_matches(", ")
                    ))?;
                }
                CommandVerbosity::VeryVerbose => {
                    buffer.write_fmt(format_args!("Sources that errorred:\n"))?;
                    for (name, url) in errored_sources.iter() {
                        buffer.write_fmt(format_args!(
                            "{name} ({url}), count: {}\n",
                            errored_sources_count.get(name).unwrap_or(&0)
                        ))?;
                    }
                }
            }
            if unknown_sources.len() > 0 {
                match verbosity {
                    CommandVerbosity::None => (),
                    CommandVerbosity::Verbose => {
                        buffer.write_fmt(format_args!(
                            "Unknown Tachiyomi/Mihon source IDs: {}\n",
                            unknown_sources
                                .iter()
                                .fold(String::new(), |mut a, s| {
                                    a.push_str(s);
                                    a.push_str(" ");
                                    a
                                })
                                .trim_end()
                        ))?;
                    }
                    CommandVerbosity::VeryVerbose => {
                        buffer.write_fmt(format_args!("Unknown Tachiyomi/Mihon source IDs:\n"))?;
                        for id in unknown_sources.iter() {
                            buffer.write_fmt(format_args!("{id}\n"))?;
                        }
                    }
                }
            }
        }
        buffer.write_fmt(format_args!(
            "Conversion completed with errors, output: {}\n",
            output_path.display()
        ))?;
        if matches!(verbosity, CommandVerbosity::Verbose) {
            buffer.write_fmt(format_args!(
                "Run command with very verbose (-V) to display ALL debug information.\n"
            ))?;
        }
    } else {
        buffer.write_fmt(format_args!(
            "{} manga successfully converted ({} ignored), output: {}\n",
            total_manga - ignored_manga,
            ignored_manga,
            output_path.display()
        ))?;
    }
    if soft_match {
        buffer.write_fmt(format_args!("[IMPORTANT] Command run with 'soft match' on; some sources may not behave as intended\n"))?;
    }

    Ok(CommandResult::Success(
        output_path.display().to_string(),
        match buffer {
            Buffer::Vector(v) => v.into_iter().map(|c| c as char).collect::<String>(),
            Buffer::Stdout(_) => String::new(),
        },
    ))
}

fn kotatsu_to_neko_manga(k: &KotatsuMangaBackup) -> nekotatsu::neko::BackupManga {
    nekotatsu::neko::BackupManga {
        source: 2499283573021220255, // Not sure if this is a volatile value
        url: k.public_url.clone(),
        title: k.title.clone(),
        artist: k.author.clone(), // Kotatsu doesn't differentiate
        author: k.author.clone(),
        status: match k.state.as_str() {
            "ONGOING" => 1,
            "FINISHED" => 2,
            "ABANDONED" => 5,
            "PAUSED" => 6,
            _ => 0,
        },
        thumbnail_url: k
            .cover_url
            .strip_suffix(".256.jpg")
            .map(str::to_string)
            .unwrap_or(k.cover_url.clone()),

        ..Default::default()
    }
}

fn kotatsu_to_neko(input_path: String, output_path: PathBuf) -> std::io::Result<CommandResult> {
    // I would at the very least like to be able to get the latest chapter and the bookmarks
    // but the process of getting the URL from the ID is not reasonably reversible as far as I can see
    println!("Note: limited support. Chapter information (including history and bookmarks) cannot be converted from Kotatsu backups.");

    let bytes = std::fs::File::open(&input_path)?;
    let mut reader = zip::read::ZipArchive::new(bytes)?;
    let mut history: Option<Vec<KotatsuHistoryBackup>> = None;
    let mut categories: Option<Vec<KotatsuCategoryBackup>> = None;
    let mut favourites: Option<Vec<KotatsuFavouriteBackup>> = None;
    // let mut bookmarks: Option<Vec<KotatsuBookmarkBackup>> = None;
    for i in 0..reader.len() {
        let file = reader.by_index(i)?;
        println!("File: {}", file.name());
        match file.name() {
            "history" => history = Some(serde_json::from_reader(file)?),
            "categories" => categories = Some(serde_json::from_reader(file)?),
            "favourites" => favourites = Some(serde_json::from_reader(file)?),
            // "bookmarks" => bookmarks = Some(serde_json::from_reader(file)?),
            _ => (),
        }
    }

    let mut neko_manga: HashMap<i64, nekotatsu::neko::BackupManga> = HashMap::new();
    let mut neko_categories: HashMap<i64, nekotatsu::neko::BackupCategory> = HashMap::new();
    if let Some(history) = history {
        for entry in history {
            if !neko_manga.contains_key(&entry.manga_id) {
                neko_manga.insert(entry.manga_id, kotatsu_to_neko_manga(&entry.manga));
            }
        }
    }
    if let Some(categories) = categories {
        for entry in categories {
            if !neko_categories.contains_key(&entry.category_id) {
                neko_categories.insert(
                    entry.category_id,
                    nekotatsu::neko::BackupCategory {
                        name: entry.title.clone(),
                        order: entry.sort_key,
                        ..Default::default()
                    },
                );
            }
        }
    }
    if let Some(favourites) = favourites {
        for entry in favourites {
            if !neko_manga.contains_key(&entry.manga_id) {
                neko_manga.insert(entry.manga_id, kotatsu_to_neko_manga(&entry.manga));
            }
            let manga = neko_manga
                .get_mut(&entry.manga_id)
                .expect("inserted if didnt exist");
            manga.categories.push(entry.category_id as i32);
        }
    }

    let backup = nekotatsu::neko::Backup {
        backup_manga: neko_manga.into_iter().map(|e| e.1).collect(),
        backup_categories: neko_categories.into_iter().map(|e| e.1).collect(),
    };
    let mut buffer = backup.encode_to_vec();
    let mut output = std::fs::File::create(output_path.clone())?;
    let mut encoder = GzEncoder::new(&mut output, Compression::fast());
    encoder.write_all(&mut buffer)?;

    println!(
        "Conversion completed successfully, output: {}",
        output_path.display()
    );

    Ok(CommandResult::Success(
        output_path.display().to_string(),
        String::new(),
    ))
}

pub fn run_command(command: Commands) -> std::io::Result<CommandResult> {
    match command {
        Commands::Update {
            kotatsu_link,
            tachi_link,
            force_download,
        } => {
            let data_path = PathBuf::from(PROJECT_DIR.data_dir());
            if !data_path.try_exists()? {
                std::fs::create_dir_all(&data_path)?;
            }
            let tachi_path = data_path.join("tachi_sources.json");
            if force_download || !tachi_path.try_exists()? {
                let response = reqwest::blocking::get(tachi_link);
                if let Ok(response) = response {
                    let text = response.text().unwrap();
                    std::fs::write(tachi_path.as_path(), text)?;
                    println!("Successfully updated extension info.");
                } else {
                    println!("Failed to download source info.");
                    return Ok(CommandResult::None);
                }
            }

            let kotatsu_path = data_path.join("kotatsu-parsers.zip");
            if force_download || !kotatsu_path.try_exists()? {
                let response = reqwest::blocking::get(kotatsu_link);
                if let Ok(mut response) = response {
                    let mut buf = Vec::new();
                    let _ = response.copy_to(&mut buf);
                    std::fs::write(kotatsu_path.as_path(), buf)?;
                    println!("Successfully downloaded parser repo.");
                } else {
                    println!("Failed to download parser repo.");
                    return Ok(CommandResult::None);
                }
            }

            kotatsu::update_parsers(kotatsu_path.as_path())?;
            println!("Successfully updated parser info.");

            Ok(CommandResult::None)
        }

        Commands::Convert {
            input,
            output,
            favorites_name,
            verbose,
            very_verbose,
            reverse,
            soft_match,
            force,
            print_output,
            config_file,
        } => {
            let conf = match config_file {
                Some(path) => {
                    let s = std::fs::read_to_string(path)?;
                    toml::from_str(&s)
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
                }
                None => config::ConfigFile::default(),
            };
            let input_path = input;
            let output_path = output.unwrap_or(if reverse {
                String::from("kotatsu_converted")
            } else {
                String::from("neko_converted")
            });
            let output_path = std::path::Path::new(&output_path)
                .with_extension("")
                .with_extension(if reverse { "tachibk" } else { "zip" });
            if !force && output_path.exists() {
                print!(
                    "File with name {} already exists; overwrite? Y(es)/N(o): ",
                    output_path.display()
                );
                io::stdout().flush()?;
                let mut buf = String::new();
                io::stdin().read_line(&mut buf)?;
                match buf.trim_end().to_lowercase().as_str() {
                    "y" | "yes" => (),
                    _ => {
                        println!("Conversion cancelled");
                        return Ok(CommandResult::None);
                    }
                }
            }

            if reverse {
                kotatsu_to_neko(input_path, output_path)
            } else {
                let verbosity = match (very_verbose, verbose) {
                    (true, _) => CommandVerbosity::VeryVerbose,
                    (_, true) => CommandVerbosity::Verbose,
                    _ => CommandVerbosity::None,
                };
                neko_to_kotatsu(
                    input_path,
                    output_path,
                    verbosity,
                    favorites_name,
                    soft_match,
                    print_output,
                    conf,
                )
            }
        }

        Commands::Debug { input } => {
            let input_path = input;
            let neko_read = decode_gzip_backup(&input_path)
                .or_else(|e| {
                    Err(match e.kind() {
                        io::ErrorKind::Interrupted | io::ErrorKind::InvalidInput => io::Error::new(std::io::ErrorKind::InvalidInput,
                            format!("Error occurred when parsing input archive, is it an actual neko backup? Original error: {e}")
                        ),
                        _ => e
                    })
                })?;

            let backup = nekotatsu::neko::Backup::decode(&mut neko_read.as_slice())?;

            println!("Manga:");
            for entry in backup.backup_manga.iter() {
                println!("{entry:?}");
            }
            println!("Categories:");
            for entry in backup.backup_categories.iter() {
                println!("{entry:?}")
            }

            Ok(CommandResult::None)
        }

        Commands::Clear | Commands::Delete => {
            #[cfg(not(target_os = "windows"))]
            let path = PROJECT_DIR.data_dir();
            #[cfg(target_os = "windows")]
            let path = PROJECT_DIR.data_dir().parent().ok_or(std::io::Error::new(
                io::ErrorKind::Other,
                "Unable to get Nekotatsu data folder path",
            ))?;

            if path.try_exists()? {
                std::fs::remove_dir_all(path)?;
                println!("Deleted directory `{}`", path.display());
            } else {
                println!("Data does not exist/is already deleted.")
            }
            Ok(CommandResult::None)
        }
    }
}
