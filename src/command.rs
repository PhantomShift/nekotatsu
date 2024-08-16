use clap::{Parser, Subcommand};
use directories::ProjectDirs;
use flate2::{write::GzEncoder, Compression};
use prost::Message;
use std::{
    collections::HashMap,
    io::{self, Write},
    path::PathBuf,
    sync::LazyLock,
};

use crate::nekotatsu_core::config::SourceFilterList;
use crate::nekotatsu_core::kotatsu::{self, *};
use crate::nekotatsu_core::*;

static PROJECT_DIR: LazyLock<ProjectDirs> =
    LazyLock::new(|| ProjectDirs::from("", "", "Nekotatsu").expect("home directory should exist"));
static DEFAULT_TACHI_SOURCE_PATH: LazyLock<PathBuf> =
    LazyLock::new(|| PROJECT_DIR.data_dir().join("tachi_sources.json").into());
static DEFAULT_KOTATSU_PARSE_PATH: LazyLock<PathBuf> =
    LazyLock::new(|| PROJECT_DIR.data_dir().join("kotatsu_parsers.json").into());

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

        /// Display some additional information
        #[arg(short, long)]
        verbose: bool,

        /// Display all debug information; overrides verbose option
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

#[derive(Debug)]
pub enum CommandVerbosity {
    None,
    Verbose,
    VeryVerbose,
}

#[derive(Debug)]
pub enum CommandResult {
    None,
    Success(String, String),
}

fn neko_to_kotatsu_command(
    input_path: String,
    output_path: PathBuf,
    verbosity: CommandVerbosity,
    favorites_name: String,
    soft_match: bool,
    print_output: bool,
    config: config::ConfigFile,
) -> std::io::Result<CommandResult> {
    let mut logger: Box<dyn Logger> = if print_output {
        Box::new(std::io::stdout())
    } else {
        Box::new(Vec::new())
    };

    let converter = MangaConverter::try_from_files(
        std::fs::File::open(&DEFAULT_KOTATSU_PARSE_PATH.as_path())?,
        std::fs::File::open(&DEFAULT_TACHI_SOURCE_PATH.as_path())?,
    )?
    .with_soft_match(soft_match);

    let backup = decode_neko_backup(std::fs::File::open(&input_path)?)?;

    let mut filter_method: Box<dyn FnMut(&extensions::SourceInfo) -> bool> =
        match (&config.whitelist, &config.blacklist) {
            // Technically whitelist and blacklist should be mutually exclusive,
            // but considering the size of this commit I'm leaving it for now
            (Some(whitelist), Some(blacklist)) => Box::new(|source| {
                !blacklist.check_source(whitelist.check_source(false, &source), &source)
            }),
            (Some(whitelist), None) => Box::new(|source| whitelist.check_source(true, &source)),
            (None, Some(blacklist)) => Box::new(|source| blacklist.check_source(true, &source)),
            (_, _) => Box::new(|_| true),
        };

    let result = converter.convert_backup(
        backup,
        &favorites_name,
        logger.as_mut(),
        filter_method.as_mut(),
    );

    let to_make = std::fs::File::create(output_path.clone())?;
    let options = zip::write::FileOptions::default();
    let mut writer = zip::ZipWriter::new(to_make);
    for (name, entry) in [
        ("history", serde_json::to_string_pretty(&result.history)),
        (
            "categories",
            serde_json::to_string_pretty(&result.categories),
        ),
        (
            "favourites",
            serde_json::to_string_pretty(&result.favourites),
        ),
        ("bookmarks", serde_json::to_string_pretty(&result.bookmarks)),
        (
            "index",
            serde_json::to_string_pretty(&[kotatsu::KotatsuIndexEntry::generate()]),
        ),
    ] {
        match entry {
            Ok(json) if json.trim() != "[]" => {
                writer.start_file(name, options)?;
                writer.write_all(json.as_bytes())?;
            }
            Ok(_) => logger.log_info(&format!("{name} is empty, ommitted from converted backup")),
            Err(e) => logger.log_info(&format!(
                "[WARNING] Error occurred processing {name}, ommitted from converted backup, original error: {e}"
            )),
        }
    }

    writer.finish()?;

    if result.errored_manga == 0 {
        logger.log_info(&format!(
            "{} manga successfully converted ({} ignored), output: {}",
            result.total_manga - result.ignored_manga,
            result.ignored_manga,
            output_path.display()
        ));
    } else {
        logger.log_info(&format!(
            "{} of {} manga and {} sources failed to convert ({} unknown).",
            result.errored_manga,
            result.total_manga,
            result.errored_sources.len(),
            result.unknown_sources.len()
        ));
        match verbosity {
            CommandVerbosity::None => {
                logger.log_info("Try running again with verbose (-v) on for details");
            }
            CommandVerbosity::Verbose => logger.log_verbose(&format!(
                "Sources that errored: {}",
                result
                    .errored_sources
                    .keys()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            CommandVerbosity::VeryVerbose => {
                logger.log_very_verbose("Sources that errorred:");
                for (name, url) in result.errored_sources.iter() {
                    logger.log_very_verbose(&format!(
                        "{name} ({url}), count: {}",
                        result.errored_sources_count.get(name).unwrap_or(&0)
                    ));
                }
            }
        }
        if result.unknown_sources.len() > 0 {
            match verbosity {
                CommandVerbosity::None => (),
                CommandVerbosity::Verbose => logger.log_verbose(&format!(
                    "Unknown Tachiyomi/Mihon source IDs: {}",
                    result
                        .unknown_sources
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
                CommandVerbosity::VeryVerbose => {
                    logger.log_very_verbose("Unknown Tachiyomi/Mihon source IDs:");
                    for id in result.unknown_sources.iter() {
                        logger.log_very_verbose(id);
                    }
                }
            }
        }

        logger.log_info(&format!(
            "Conversion completed with errors, output: {}",
            output_path.display()
        ));
        if let CommandVerbosity::Verbose = verbosity {
            logger
                .log_verbose("Run command with very verbose (-V) to display ALL debug information.")
        }
    }

    if soft_match {
        logger.log_info(
            "[IMPORTANT] Command run with 'soft match' on; some sources may not behave as intended",
        )
    }

    Ok(CommandResult::Success(
        output_path.display().to_string(),
        logger.capture_output(),
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
                if let Ok(response) = response {
                    let b = response
                        .bytes()
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                    std::fs::write(kotatsu_path.as_path(), b)?;
                    println!("Successfully downloaded parser repo.");
                } else {
                    println!("Failed to download parser repo.");
                    return Ok(CommandResult::None);
                }
            }

            let new_data = std::fs::File::open(&kotatsu_path)?;
            let save_to = std::fs::File::create(&DEFAULT_KOTATSU_PARSE_PATH.as_path())?;

            kotatsu::update_parsers(&new_data, &save_to)?;
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
                // neko_to_kotatsu(
                neko_to_kotatsu_command(
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
            let backup = decode_neko_backup(std::fs::File::open(&input)?)?;

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
