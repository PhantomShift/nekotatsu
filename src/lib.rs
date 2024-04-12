use std::{collections::{HashMap, HashSet}, io::{self, Read, Write}, path::PathBuf, sync::Mutex};

use flate2::{write::GzEncoder, Compression};
use once_cell::sync::OnceCell;
use prost::Message;
use clap::{Parser, Subcommand};
use directories::ProjectDirs;
use lazy_static::lazy_static;

pub mod extensions;
pub mod nekotatsu {
    pub mod neko {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/neko.backup.rs"));
    }
}
pub mod kotatsu;
use kotatsu::*;

#[cfg(feature="gui")]
pub mod gui;
#[cfg(feature="gui")]
pub mod child_window;

use crate::extensions::get_source;

lazy_static!{
    static ref PROJECT_DIR: ProjectDirs = ProjectDirs::from("", "", "Nekotatsu").expect("Invalid application directories generated");
    static ref TACHI_SOURCE_PATH: PathBuf = PathBuf::from(PROJECT_DIR.data_dir()).join("tachi_sources.json");
    static ref KOTATSU_PARSE_PATH: PathBuf = PathBuf::from(PROJECT_DIR.data_dir()).join("kotatsu_parsers.json");
}

const CATEGORY_DEFAULT: i64 = 2;
const CATEGORY_OFFSET: i64 = CATEGORY_DEFAULT + 1;

/// Simple CLI tool that converts Neko backups into Kotatsu backups
#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<Commands>
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
        #[arg(short, long, default_value_t = String::from("Library"))]
        favorites_name: String,

        /// Display some additional information
        #[arg(short, long)]
        verbose: bool,

        /// Convert to Neko instead
        #[arg(short, long)]
        reverse: bool,

        /// Strip top-level domains when comparing Tachiyomi/Mihon sources to Kotatsu parsers
        #[arg(short, long)]
        soft_match: bool,

        /// Convert without asking about overwriting existing files
        #[arg(short, long)]
        force: bool,

        #[arg(long, hide=true, default_value_t=true)]
        print_output: bool
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
    Debug {
        input: String
    },

    /// Deletes any files downloaded by nekotatsu (the data directory);
    /// Effectively the same as running `rm -rf ~/.local/share/nekotatsu` on Linux and `rmdir /s /q %APPDATA%\Nekotatsu` on Windows.
    Clear,
    /// Alias for `clear`
    Delete
}

pub enum CommandResult {
    None,
    Success(String, String)
}

fn manga_get_source_name(manga: &nekotatsu::neko::BackupManga, soft_match: bool) -> String {
    static SOURCES: OnceCell<Mutex<HashMap<i64, String>>> = OnceCell::new();
    static KOTATSU_PARSER_LIST: OnceCell<Vec<KotatsuParser>> = OnceCell::new();

    match manga.source {
        // Hardcoded (known tachi IDs)
        2499283573021220255 => "MANGADEX".to_owned(),
        1998944621602463790 => "MANGAPLUSPARSER_EN".to_owned(),

        // Other online sources
        _ => {
            let sources = SOURCES.get_or_init(|| Mutex::new(HashMap::new()));
            let mut lock = sources.lock().unwrap();
            let source = lock.entry(manga.source).or_insert_with(|| {
                let parser_list = KOTATSU_PARSER_LIST.get_or_try_init(|| {
                    let list = std::fs::read_to_string(KOTATSU_PARSE_PATH.as_path())?;
                    let r: Result<Vec<KotatsuParser>, serde_json::error::Error> = serde_json::from_str(&list);
                    r.map_err(|_e| {
                        io::Error::new(io::ErrorKind::InvalidData, "Error reading Kotatsu parser list")
                    })
                });

                match (parser_list, extensions::get_source(manga.source)) {
                    (Ok(parser_list), Ok(source)) => {
                        let urls = vec![
                            source.baseUrl.trim_start_matches("http://").trim_start_matches("https://").to_string(),
                            source.baseUrl.trim_start_matches("http://").trim_start_matches("https://").trim_start_matches("www.").to_string(),
                        ];
                        parser_list.iter().find(|p| {
                            (p.name.to_lowercase() == source.name) || {
                                p.domains.iter().any(|d| {
                                    urls.iter().any(|url| d == url)
                                })
                            }
                        }).or(soft_match.then_some({
                            // Boldly assuming that there's only one relevant top-level domain
                            let url = source.baseUrl.trim_start_matches("http://").trim_start_matches("https://");
                            match url.rsplit_once(".") {
                                Some((name, _tld)) => {
                                    parser_list.iter().find(|p| {
                                        p.domains.iter().any(|d| d.contains(name))
                                    })
                                },
                                None => None
                            }
                            
                        }).flatten()).map_or(String::from("UNKNOWN"), |p| p.name.clone())
                    }
                    _ => String::from("UNKNOWN")
                }
            });

            source.clone()
        }
    }
}

fn manga_to_kotatsu(manga: &nekotatsu::neko::BackupManga, soft_match: bool) -> Result<KotatsuMangaBackup, io::Error> {
    let source_info = extensions::get_source(manga.source)?;
    let domain = source_info.baseUrl;
    let source_name = manga_get_source_name(manga, soft_match);
    let relative_url = kotatsu::correct_url(&source_name, &manga.url);
    // value used when calling generateUid
    let manga_identifier = kotatsu::correct_identifier(&source_name, &relative_url);

    let kotatsu_manga = KotatsuMangaBackup {
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
            _ => ""
        }),
        source: source_name.clone(),
        tags: [],
    };

    return Ok(kotatsu_manga);
}

fn decode_gzip_backup(path: &str) -> std::io::Result<Vec<u8>> {
    let bytes = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(bytes);
    let mut decoder = flate2::read::GzDecoder::new(&mut reader);
    let mut buf = Vec::new();
    decoder.read_to_end(&mut buf)?;

    return Ok(buf)
}

fn neko_to_kotatsu(input_path: String, output_path: PathBuf, verbose: bool, favorites_name: String, soft_match: bool, print_output: bool) -> std::io::Result<CommandResult> {
    let mut my_vec = Vec::new();
    let mut buffer: Box<dyn Write> = if print_output {
        Box::new(std::io::stdout())
    } else {
        Box::new(&mut my_vec)
    };

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

    result_categories.push(KotatsuCategoryBackup {
        category_id: CATEGORY_DEFAULT,
        created_at: 0,
        sort_key: 0,
        title: favorites_name,
        order: Some(String::from("NAME")),
        track: Some(true),
        show_in_lib: Some(true),
        deleted_at: 0
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
            deleted_at: 0
        });
    }

    for manga in backup.backup_manga.iter() {
        total_manga += 1;

        // ignore locally imported manga
        if manga.source == 0 {
            if verbose {
                buffer.write_fmt(format_args!("[WARNING] Unable to convert '{}', local manga currently unsupported", manga.title))?;
            }
            errored_manga += 1;
            continue;
        }

        let kotatsu_manga = manga_to_kotatsu(&manga, soft_match)?;

        if kotatsu_manga.source == "UNKNOWN" {
            let source = get_source(manga.source);
            if let Ok(source) = source {
                if verbose {
                    if source.name == "Unknown" {
                        buffer.write_fmt(format_args!("[WARNING] Unable to convert '{}', unknown Tachiyomi/Mihon source (ID: {})", manga.title, source.id))?;
                    } else {
                        buffer.write_fmt(format_args!("[WARNING] Unable to convert '{}' from source {} ({}), Kotatsu parser not found", manga.title, source.name, source.baseUrl))?;
                    }
                }
                errored_sources.insert(source.name.clone(), source.baseUrl);
                errored_sources_count.entry(source.name.clone()).and_modify(|e| *e += 1).or_insert(1);
                if source.name == "Unknown" {
                    unknown_sources.insert(source.id);
                }
            } else if verbose {
                buffer.write_fmt(format_args!("[WARNING] Unable to convert '{}', unknown Tachiyomi source (ID {})", manga.title, manga.source))?;
            }
            errored_manga += 1;
            continue;
        }

        let make_fav_backup = |id: i64| {
            KotatsuFavouriteBackup {
                manga_id: kotatsu_manga.id.clone(),
                category_id: id,
                sort_key: 0,
                created_at: 0,
                deleted_at: 0,
                manga: kotatsu_manga.clone()
            }
        };
        for category_id in manga.categories.iter() {
            result_favourites.push(make_fav_backup(*category_id as i64 + CATEGORY_OFFSET))
        }
        result_favourites.push(make_fav_backup(CATEGORY_DEFAULT));

        let latest_chapter = manga.chapters.iter().fold(None, |current, checking| {
            match current {
                None if checking.read => Some(checking),
                Some(current) if checking.read && checking.chapter_number > current.chapter_number => Some(checking),
                _ => current
            }
        });
        let bookmarks = manga.chapters.iter().filter_map(|chapter| {
            if !chapter.bookmark {return None}
            Some(KotatsuBookmarkEntry {
                manga_id: kotatsu_manga.id,
                page_id: 0,
                chapter_id: get_kotatsu_id(&kotatsu_manga.source, &correct_identifier(&kotatsu_manga.source, &chapter.url)),
                page: chapter.last_page_read,
                scroll: 0,
                image_url: kotatsu_manga.cover_url.clone(),
                created_at: 0,
                percent: if chapter.last_page_read + chapter.pages_left == 0 { 
                    0.0
                } else {
                    chapter.last_page_read as f32 / (chapter.last_page_read as f32 + chapter.pages_left as f32)
                },
            })
        }).collect::<Vec<_>>();
        if bookmarks.len() > 0 {
            result_bookmarks.push(KotatsuBookmarkBackup {
                manga: kotatsu_manga.clone(),
                tags: [],
                bookmarks
            })
        }
        let newest_cached_chapter = manga.chapters.iter().max_by(|a, b| a.chapter_number.total_cmp(&b.chapter_number));
        let last_read = manga.history.iter().max_by(|l, r| l.last_read.cmp(&r.last_read)).map(|entry| entry.last_read).unwrap_or(manga.last_update);
        let kotatsu_history = KotatsuHistoryBackup {
            manga_id: kotatsu_manga.id.clone(),
            created_at: manga.date_added,
            updated_at: last_read,
            chapter_id: if let Some(latest) = latest_chapter {
                get_kotatsu_id(&kotatsu_manga.source, &correct_identifier(&kotatsu_manga.source, &latest.url))
            } else {0},
            page: if let Some(latest) = latest_chapter {latest.last_page_read} else {0},
            scroll: 0.0,
            percent: match (latest_chapter, newest_cached_chapter) {
                (Some(latest), Some(newest)) if latest.chapter_number > 0.0 => {
                    (latest.chapter_number - 1.0) / newest.chapter_number
                }
                _ => 0.0
            },
            manga: kotatsu_manga
        };
        result_history.push(kotatsu_history);
    }

    let to_make = std::fs::File::create(output_path.clone())?;
    let options = zip::write::FileOptions::default();
    let mut writer = zip::ZipWriter::new(to_make);
    for (name, entry) in [
        ("history", serde_json::to_string_pretty(&result_history)),
        ("categories", serde_json::to_string_pretty(&result_categories)),
        ("favourites", serde_json::to_string_pretty(&result_favourites)),
        ("bookmarks", serde_json::to_string_pretty(&result_bookmarks)),
        ("index", serde_json::to_string_pretty(&[kotatsu::KotatsuIndexEntry::generate()]))
    ] {
        match entry {
            Ok(json) => if json.trim_end() != "[]" {
                writer.start_file(name, options)?;
                writer.write_all(json.as_bytes())?;
            }
            #[allow(unreachable_patterns)]
            Ok(_) => {
                buffer.write_fmt(format_args!("{name} is empty, ommitted from converted backup\n"))?;
            },
            Err(e) => {
                buffer.write_fmt(format_args!("Warning: Error occured processing {name}, ommitted from converted backup\n"))?;
                buffer.write_fmt(format_args!("Original error: {e}\n"))?;
            }
        }
    }
    writer.finish()?;

    if errored_manga > 0 {
        buffer.write_fmt(format_args!("{errored_manga} of {total_manga} manga and {} sources failed to convert ({} unknown).\n", errored_sources.len(), unknown_sources.len()))?;
        if !verbose {
            buffer.write_fmt(format_args!("Try running again with verbose (-v) on for details\n"))?;
        } else {
            buffer.write_fmt(format_args!("Sources that errorred:"))?;
            for (name, url) in errored_sources.iter() {
                buffer.write_fmt(format_args!("{name} ({url}), count: {}", errored_sources_count.get(name).unwrap_or(&0)))?;
            }
            if unknown_sources.len() > 0 {
                buffer.write_fmt(format_args!("Unknown Tachiyomi/Mihon source IDs:"))?;
                for id in unknown_sources.iter() {
                    buffer.write_fmt(format_args!("{id}"))?;
                }
            }
        }
        buffer.write_fmt(format_args!("Conversion completed with errors, output: {}\n", output_path.display()))?;
    } else {
        buffer.write_fmt(format_args!("{total_manga} manga successfully converted, output: {}\n", output_path.display()))?;
    }
    if soft_match {
        println!("[IMPORTANT] Command run with 'soft match' on; some sources may not behave as intended")
    }

    drop(buffer);
    
    Ok(CommandResult::Success(
        output_path.display().to_string(),
        if print_output {
            String::new()
        } else {
            my_vec.into_iter().map(|c| c as char).collect::<String>()
        }
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
            _ => 0
        },
        thumbnail_url: k.cover_url.strip_suffix(".256.jpg").map(str::to_string).unwrap_or(k.cover_url.clone()),

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
            _ => ()
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
                neko_categories.insert(entry.category_id, nekotatsu::neko::BackupCategory {
                    name: entry.title.clone(), 
                    order: entry.sort_key, 
                    ..Default::default()
                });
            }
        }
    }
    if let Some(favourites) = favourites {
        for entry in favourites {
            if !neko_manga.contains_key(&entry.manga_id) {
                neko_manga.insert(entry.manga_id, kotatsu_to_neko_manga(&entry.manga));
            }
            let manga = neko_manga.get_mut(&entry.manga_id).expect("inserted if didnt exist");
            manga.categories.push(entry.category_id as i32);
        }
    }

    let backup = nekotatsu::neko::Backup {
        backup_manga: neko_manga.into_iter().map(|e|e.1).collect(),
        backup_categories: neko_categories.into_iter().map(|e|e.1).collect()
    };
    let mut buffer = backup.encode_to_vec();
    let mut output = std::fs::File::create(output_path.clone())?;
    let mut encoder = GzEncoder::new(&mut output, Compression::fast());
    encoder.write_all(&mut buffer)?;

    println!("Conversion completed successfully, output: {}", output_path.display());

    Ok(CommandResult::Success(output_path.display().to_string(), String::new()))
}

pub fn run_command(command: Commands) -> std::io::Result<CommandResult> {
    match command {
        Commands::Update { kotatsu_link, tachi_link , force_download} => {
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

        Commands::Convert { input, output, favorites_name, verbose, reverse, soft_match, force, print_output } => {

            let input_path = input;
            let output_path = output.unwrap_or(if reverse {
                String::from("kotatsu_converted")
            } else {
                String::from("neko_converted")
            });
            let output_path = std::path::Path::new(&output_path).with_extension("").with_extension(if reverse {
                "tachibk"
            } else {
                "zip"
            });
            if !force && output_path.exists() {
                print!("File with name {} already exists; overwrite? Y(es)/N(o): ", output_path.display());
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
                neko_to_kotatsu(input_path, output_path, verbose, favorites_name, soft_match, print_output)
            }
        },

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

            println!("{backup:?}");

            Ok(CommandResult::None)
        }

        Commands::Clear | Commands::Delete => {
            
            #[cfg(not(target_os = "windows"))]
            let path = PROJECT_DIR.data_dir();
            #[cfg(target_os = "windows")]
            let path = PROJECT_DIR.data_dir().parent()
                .ok_or(std::io::Error::new(io::ErrorKind::Other, "Unable to get Nekotatsu data folder path"))?;

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
