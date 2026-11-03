//! Данные дерева социальных институтов (`data/culture/social_*.json`):
//! загрузка по эпохам и константа раскладки эпох.

use bevy::prelude::*;
use serde::Deserialize;

use super::tree_view::{json_files, read_json};

#[derive(Deserialize, Default, Resource, Debug)]
pub(crate) struct SocialData {
    #[serde(default)]
    pub(crate) institutions: Vec<SocialEntry>,
}

#[allow(unused)]
#[derive(Deserialize, Default, Debug, Clone)]
pub(crate) struct SocialEntry {
    pub(crate) id: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) era: String,
    #[serde(default)]
    pub(crate) requires: Vec<String>,
    #[serde(default)]
    pub(crate) description: String,
}

/// То же раскладочное правило эпох, что у технологий, для дерева
/// социальных институтов (`data/culture/`).
pub(crate) const SOCIAL_ERA_COLUMNS: [usize; 4] = [8, 8, 8, 8];

/// Загружает все файлы институтов эпохи и сливает их в один набор.
pub(crate) fn load_social_dir(dir: &str) -> SocialData {
    let mut data = SocialData::default();
    for path in json_files(dir) {
        match read_json::<SocialData>(&path) {
            Ok(mut part) => data.institutions.append(&mut part.institutions),
            Err(e) => warn!("Институты: {e}"),
        }
    }
    debug!("Загружено институтов: {}", data.institutions.len());
    data
}
