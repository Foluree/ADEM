//! Данные дерева технологий (`data/science/tech_*.json`): загрузка по
//! эпохам и константа раскладки эпох.

use bevy::prelude::*;
use serde::Deserialize;

use super::tree_view::{json_files, read_json};

#[derive(Deserialize, Default, Resource, Debug)]
pub(crate) struct TechData {
    #[serde(default)]
    pub(crate) technologies: Vec<TechEntry>,
}

#[allow(unused)]
#[derive(Deserialize, Default, Debug, Clone)]
pub(crate) struct TechEntry {
    pub(crate) id: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) era: String,
    #[serde(default)]
    pub(crate) requires: Vec<String>,
    #[serde(default)]
    pub(crate) description: String,
}

/// Сколько колонок (последовательных карточек) занимает каждая эпоха в
/// дереве технологий — порядок эпох: каменный, бронзовый, железный век,
/// средневековье. Разделители рисуются после каждой границы, кроме
/// последней. Значение правится вместе с данными `data/science/`.
pub(crate) const TECH_ERA_COLUMNS: [usize; 4] = [10, 10, 10, 10];

/// Загружает все файлы технологий эпохи и сливает их в один набор.
pub(crate) fn load_tech_dir(dir: &str) -> TechData {
    let mut data = TechData::default();
    for path in json_files(dir) {
        match read_json::<TechData>(&path) {
            Ok(mut part) => data.technologies.append(&mut part.technologies),
            Err(e) => warn!("Технологии: {e}"),
        }
    }
    debug!("Загружено технологий: {}", data.technologies.len());
    data
}
