use serde::{Serialize, Deserialize};

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

pub fn get_kotatsu_id(source_name: &str, url: &str) -> i64 {
    let mut id: i64 = 1125899906842597;
    source_name.chars().for_each(|c| id = (31i64.overflowing_mul(id)).0.overflowing_add(c as i64).0);
    url.chars().for_each(|c| id = (31i64.overflowing_mul(id)).0.overflowing_add(c as i64).0);
    return id
}