use std::{io::{self, Read, Write}, path::PathBuf, collections::HashMap};

use flate2::{write::GzEncoder, Compression};
use prost::Message;

pub mod kotatsu;
use kotatsu::*;

pub mod nekotatsu {
    pub mod neko {
        include!(concat!(env!("OUT_DIR"), "/neko.backup.rs"));
    }
}

fn decode_gzip_backup(path: &str) -> std::io::Result<Vec<u8>> {
    let bytes = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(bytes);
    let mut decoder = flate2::read::GzDecoder::new(&mut reader);
    let mut buf = Vec::new();
    decoder.read_to_end(&mut buf)?;

    return Ok(buf)
}

fn neko_to_kotatsu(input_path: String, output_path: PathBuf) -> std::io::Result<()> {
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
    let mut result_categories = Vec::new();
    let mut result_favourites = Vec::new();
    let mut result_history = Vec::new();
    let mut result_bookmarks = Vec::new();
    for (id, category) in backup.backup_categories.iter().enumerate() {
        result_categories.push(KotatsuCategoryBackup {
            // kotatsu appears to not allow index 0 for category id
            category_id: id as i64 + 1,
            created_at: 0,
            sort_key: category.order,
            title: category.name.clone(),
            order: None,
            track: None,
            show_in_lib: None,
            deleted_at: 0
        });
    }

    for manga in backup.backup_manga.iter() {
        let manga_url = manga.url.replace("/manga/", "");
        let kotatsu_manga = KotatsuMangaBackup {
            id: get_kotatsu_id("MANGADEX", &manga_url),
            title: manga.title.clone(),
            alt_tile: None,
            url: manga_url.clone(),
            public_url: format!("https://mangadex.org/title/{manga_url}"),
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
            source: String::from("MANGADEX"),
            tags: [],
        };
        if manga.categories.len() > 0 {
            for category_id in manga.categories.iter() {
                result_favourites.push(KotatsuFavouriteBackup {
                    manga_id: kotatsu_manga.id.clone(),
                    category_id: *category_id as i64 + 1,
                    sort_key: 0,
                    created_at: 0,
                    deleted_at: 0,
                    manga: kotatsu_manga.clone()
                });
            }
        }
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
                chapter_id: get_kotatsu_id("MANGADEX", &chapter.url.replace("/chapter/", "")),
                page: chapter.last_page_read,
                scroll: 0,
                image_url: kotatsu_manga.cover_url.clone(),
                created_at: 0,
                percent: chapter.last_page_read as f32 / (chapter.last_page_read as f32 + chapter.pages_left as f32),
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
        if last_read != 0 {
            println!("{}", kotatsu_manga.id)
        }
        let kotatsu_history = KotatsuHistoryBackup {
            manga_id: kotatsu_manga.id.clone(),
            created_at: manga.date_added,
            // updated_at: manga.last_update,
            updated_at: last_read,
            chapter_id: if let Some(latest) = latest_chapter {
                get_kotatsu_id("MANGADEX", &latest.url.replace("/chapter/", ""))
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
    ] {
        match entry {
            Ok(json) => if json.trim_end() != "[]" {
                writer.start_file(name, options)?;
                writer.write_all(json.as_bytes())?;
            }
            #[allow(unreachable_patterns)]
            Ok(_) => println!("{name} is empty, ommitted from converted backup"),
            Err(e) => {
                println!("Warning: Error occured processing {name}, ommitted from converted backup");
                println!("Original error: {e}");
            }
        }
    }
    writer.finish()?;

    println!("Conversion completed successfully, output: {}", output_path.display());
    Ok(())
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

fn kotatsu_to_neko(input_path: String, output_path: PathBuf) -> std::io::Result<()> {
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

    Ok(())
}

fn main() -> std::io::Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() < 2 {
        println!("Usage: {} (input neko.tachibk) (optional output name)", args[0]);
        return Ok(())
    }
    let reverse = args.contains(&String::from("-r")) || args.contains(&String::from("--reverse"));

    let input_path = args[1].to_owned();
    let output_path = args.get(2).map(String::to_owned).unwrap_or(if reverse {
        String::from("kotatsu_converted")
    } else {
        String::from("neko_converted")
    });
    let output_path = std::path::Path::new(&output_path).with_extension("").with_extension(if reverse {
        "tachibk"
    } else {
        "zip"
    });
    if output_path.exists() {
        print!("File with name {} already exists; overwrite? Y(es)/N(o): ", output_path.display());
        io::stdout().flush()?;
        let mut buf = String::new();
        io::stdin().read_line(&mut buf)?;
        match buf.trim_end().to_lowercase().as_str() {
            "y" | "yes" => (),
            _ => {
                println!("Conversion cancelled");
                return Ok(());
            }
        }
    }

    if reverse {
        kotatsu_to_neko(input_path, output_path)
    } else {
        neko_to_kotatsu(input_path, output_path)
    }
}