use std::io::{self, Read, Write};

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

fn main() -> std::io::Result<()> {
    if std::env::args().len() < 2 {
        println!("Usage: {} (input neko.tachibk) (optional output name)", std::env::args().nth(0).expect("executable should exist by definition"));
        return Ok(())
    }
    let input_path = std::env::args().nth(1).unwrap();
    let output_path = std::env::args().nth(2).unwrap_or(String::from("neko_converted"));
    let output_path = std::path::Path::new(&output_path).with_extension("").with_extension("zip");
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
            category_id: id as u32 + 1,
            created_at: 0,
            sort_key: category.order as u32,
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
                1 => "ongoing",
                2 | 4 => "completed",
                5 => "cancelled",
                6 => "hiatus",
                _ => "unknown"
            }),
            source: String::from("MANGADEX"),
            tags: [],
        };
        if manga.categories.len() > 0 {
            for category_id in manga.categories.iter() {
                result_favourites.push(KotatsuFavouriteBackup {
                    manga_id: kotatsu_manga.id.clone(),
                    category_id: *category_id as u64 + 1,
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
                page: chapter.last_page_read as u32,
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
        let kotatsu_history = KotatsuHistoryBackup {
            manga_id: kotatsu_manga.id.clone(),
            created_at: manga.date_added as u64,
            updated_at: manga.last_update as u64,
            chapter_id: if let Some(latest) = latest_chapter {
                get_kotatsu_id("MANGADEX", &latest.url.replace("/chapter/", ""))
            } else {0},
            page: if let Some(latest) = latest_chapter {latest.last_page_read as u32} else {0},
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
            Ok(_) => println!("{name} is empted, ommitted from converted backup"),
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