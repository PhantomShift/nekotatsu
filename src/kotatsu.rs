use serde::Serialize;

#[derive(Debug, Serialize, Clone)]
pub struct KotatsuMangaBackup {
    pub id: u64,
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

#[derive(Debug, Serialize)]
pub struct KotatsuHistoryBackup {
    pub manga_id: u64,
    pub created_at: u64,
    pub updated_at: u64,
    pub chapter_id: u64,
    pub page: u32,
    pub scroll: f32,
    pub percent: f32,
    pub manga: KotatsuMangaBackup,
}

#[derive(Debug, Serialize)]
pub struct KotatsuCategoryBackup {
    pub category_id: u32,
    pub created_at: u64,
    pub sort_key: u32,
    pub title: String,
    pub order: Option<String>,
    pub track: Option<bool>,
    pub show_in_lib: Option<bool>,
    pub deleted_at: u64,
}

#[derive(Debug, Serialize)]
pub struct KotatsuFavouriteBackup {
    pub manga_id: u64,
    pub category_id: u64,
    pub sort_key: u32,
    pub created_at: u64,
    pub deleted_at: u64,
    pub manga: KotatsuMangaBackup
}

#[derive(Debug, Serialize)]
pub struct KotatsuBookmarkBackup {
    pub manga: KotatsuMangaBackup,
    pub tags: [String;0],
    pub bookmarks: Vec<KotatsuBookmarkEntry>
}
#[derive(Debug, Serialize)]
pub struct KotatsuBookmarkEntry {
    pub manga_id: u64,
    pub page_id: u64,
    pub chapter_id: u64,
    pub page: u32,
    pub scroll: u32,
    pub image_url: String,
    pub created_at: u64,
    pub percent: f32
}

pub fn get_kotatsu_id(source_name: &str, url: &str) -> u64 {
    let mut id: u64 = 1125899906842597;
    source_name.chars().for_each(|c| id = (31u64.overflowing_mul(id)).0.overflowing_add(c as u64).0);
    url.chars().for_each(|c| id = (31u64.overflowing_mul(id)).0.overflowing_add(c as u64).0);
    return id as u64
}