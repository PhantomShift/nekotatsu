use clap::{Parser, Subcommand};
use etcetera::{app_strategy::AppStrategy, AppStrategyArgs};
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

#[cfg(target_os = "windows")]
type AppPathStrategy = etcetera::app_strategy::Windows;
#[cfg(target_os = "macos")]
type AppPathStrategy = etcetera::app_strategy::Apple;
#[cfg(not(target_os = "windows"))]
#[cfg(not(target_os = "macos"))]
type AppPathStrategy = etcetera::app_strategy::Xdg;

static APP_PATH: LazyLock<AppPathStrategy> = LazyLock::new(|| {
    etcetera::app_strategy::choose_native_strategy(AppStrategyArgs {
        app_name: "Nekotatsu".to_string(),
        ..Default::default()
    })
    .expect("application paths should be findable on all used platforms")
});
static DEFAULT_TACHI_SOURCE_PATH: LazyLock<PathBuf> =
    LazyLock::new(|| APP_PATH.data_dir().join("tachi_sources.json"));
static DEFAULT_KOTATSU_PARSE_PATH: LazyLock<PathBuf> =
    LazyLock::new(|| APP_PATH.data_dir().join("kotatsu_parsers.json"));
static DEFAULT_SCRIPT_PATH: LazyLock<PathBuf> =
    LazyLock::new(|| APP_PATH.data_dir().join("correction.luau"));

enum PathType {
    Url(reqwest::Url),
    Filesystem(PathBuf),
}

impl TryFrom<&str> for PathType {
    type Error = std::io::Error;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let path = {
            if let Ok(url) = reqwest::Url::parse(value) {
                if let Ok(path) = url.to_file_path() {
                    path
                } else {
                    return Ok(PathType::Url(url));
                }
            } else {
                PathBuf::from(value)
            }
        };
        match path.canonicalize() {
            Ok(path) => Ok(PathType::Filesystem(path)),
            Err(e) => Err(e),
        }
    }
}

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
        /// Download URL or file path for Kotatsu parsers repo.
        #[arg(short, long, default_value_t = String::from("https://github.com/KotatsuApp/kotatsu-parsers/archive/refs/heads/master.zip"))]
        kotatsu_link: String,

        /// Download URL or file path for Tachiyomi extension json list (minified)
        #[arg(short, long, default_value_t = String::from("https://raw.githubusercontent.com/keiyoushi/extensions/repo/index.min.json"))]
        tachi_link: String,

        /// Download URL or ifle path for correction script
        #[arg(short, long, default_value_t = String::from("https://raw.githubusercontent.com/phantomshift/nekotatsu/master/nekotatsu-core/src/correction.luau"))]
        script_link: String,

        /// Force download of all files even if they already exist
        #[arg(short, long)]
        force_download: bool,

        /// Force download/copy of Kotatsu parsers repo
        #[arg(visible_alias("fk"), long)]
        force_kotatsu: bool,

        /// Force download/copy of Tachiyomi extensions list
        #[arg(visible_alias("ft"), long)]
        force_tachi: bool,

        /// Force download/copy of correction script
        #[arg(visible_alias("fs"), long)]
        force_script: bool,
    },

    /// Deletes any files downloaded by nekotatsu (the data directory);
    /// Effectively the same as running `rm -rf ~/.local/share/nekotatsu` on Linux and `rmdir /s /q %APPDATA%\Nekotatsu` on Windows.
    Clear,
    /// Alias for `clear`
    Delete,

    /// Output backup info
    #[command(hide(true))]
    Debug { input: String },

    #[command(hide(true))]
    Filter {
        input: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(short, long, value_delimiter('\n'))]
        filter_ids: Vec<i64>,
    },
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
    .with_soft_match(soft_match)
    .with_runtime(
        match script_interface::ScriptRuntime::create(&DEFAULT_SCRIPT_PATH.as_path()) {
            Ok(runtime) => runtime,
            Err(err) => {
                logger.log_info(&format!("[WARNING] Error loading downloaded script, falling back to default implementation, which may be outdated. Did you run the update command? Original error: {err:?}",));
                script_interface::ScriptRuntime::default()
            }
        }
    );

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
    let options = zip::write::FileOptions::<()>::default();
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
            script_link,
            force_download,
            force_kotatsu,
            force_tachi,
            force_script,
        } => {
            let data_path = APP_PATH.data_dir();
            if !data_path.try_exists()? {
                std::fs::create_dir_all(&data_path)?;
            }

            let attempt_download = |destination: &str,
                                    from: &str,
                                    force: bool,
                                    success_message: &str,
                                    failure_message: &str|
             -> std::io::Result<PathBuf> {
                let output_path = data_path.join(destination);
                if force || force_download || !output_path.try_exists()? {
                    match PathType::try_from(from) {
                        Ok(PathType::Filesystem(path)) => {
                            std::fs::copy(&path, &output_path)?;
                            println!("Copied {} to {}", path.display(), output_path.display());
                        }
                        Err(e) => {
                            println!("Error getting file: if this is a url, did you include 'https://'? Original error: {e:?}");
                        }

                        Ok(PathType::Url(url)) => {
                            let response = reqwest::blocking::get(url);
                            match response {
                                Ok(response) if response.status().is_success() => {
                                    let b = response
                                        .bytes()
                                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                                    std::fs::write(&output_path, b)?;
                                    println!("{success_message}")
                                }
                                Ok(failed) => {
                                    println!("{failure_message}");
                                    println!("Response: {}", failed.status(),);
                                }
                                Err(err) => {
                                    println!("{failure_message}");
                                    println!("Error: {err:#?}");
                                }
                            }
                        }
                    };
                }
                Ok(output_path)
            };

            attempt_download(
                "tachi_sources.json",
                &tachi_link,
                force_tachi,
                "Successfully updated extension info.",
                "Failed to download source info.",
            )?;

            attempt_download(
                "kotatsu-parsers.zip",
                &kotatsu_link,
                force_kotatsu,
                "Successfully downloaded parser repo.",
                "Failed to download parser repo.",
            )
            .and_then(|kotatsu_path| {
                let new_data = std::fs::File::open(&kotatsu_path)?;
                let save_to = std::fs::File::create(&DEFAULT_KOTATSU_PARSE_PATH.as_path())?;
                kotatsu::update_parsers(&new_data, &save_to)?;
                Ok(kotatsu_path)
            })?;

            println!("Successfully updated parser info.");

            attempt_download(
                "correction.luau",
                &script_link,
                force_script,
                "Successfully downloaded correction script.",
                "Failed to download correction script.",
            )?;

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
            // TODO: Category sorting method override, automatically detect if it should use default from filename
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

        Commands::Clear | Commands::Delete => {
            let path = APP_PATH.data_dir();
            #[cfg(target_os = "windows")]
            let path = path.parent().ok_or(std::io::Error::new(
                io::ErrorKind::Other,
                "Unable to get Nekotatsu data folder path",
            ))?;

            if path.try_exists()? {
                std::fs::remove_dir_all(&path)?;
                println!("Deleted directory `{}`", path.display());
            } else {
                println!("Data does not exist/is already deleted.")
            }
            Ok(CommandResult::None)
        }

        Commands::Debug { input } => {
            let backup = decode_neko_backup(std::fs::File::open(&input)?)?;

            println!("Manga:");
            for entry in backup.backup_manga.iter() {
                println!("{entry:#?}");
            }
            println!("Categories:");
            for entry in backup.backup_categories.iter() {
                println!("{entry:#?}")
            }

            Ok(CommandResult::None)
        }

        Commands::Filter {
            input,
            output,
            filter_ids,
        } => {
            let backup = decode_neko_backup(std::fs::File::open(&input)?)?;

            let filter_ids: std::collections::HashSet<i64, std::hash::RandomState> =
                std::collections::HashSet::from_iter(filter_ids.into_iter());

            let filtered: Vec<nekotatsu::neko::BackupManga> = backup
                .backup_manga
                .iter()
                .filter_map(|manga| {
                    if filter_ids.contains(&manga.source) {
                        // Compat: deserialization fails in app if
                        // last_read is 0
                        let history = manga
                            .history
                            .iter()
                            .filter_map(|h| (h.last_read != 0).then_some(h.clone()))
                            .collect();
                        Some(nekotatsu::neko::BackupManga {
                            history,
                            ..manga.clone()
                        })
                    } else {
                        None
                    }
                })
                .collect();

            let filtered = nekotatsu::neko::Backup {
                backup_manga: filtered,
                ..backup
            };

            let mut buffer = Vec::new();
            filtered.encode(&mut buffer)?;

            let output_path = std::path::Path::new(&output.unwrap_or(input))
                .with_extension("")
                .with_extension("filtered.tachibk");

            let output_file = std::fs::File::create(&output_path)?;
            let mut writer = GzEncoder::new(output_file, Compression::best());

            writer.write_all(&mut buffer)?;
            writer.finish()?;

            println!("Filtered successfully, output: {}", output_path.display());

            Ok(CommandResult::None)
        }
    }
}
