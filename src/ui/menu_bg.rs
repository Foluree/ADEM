//! Фон полноэкранных меню деревьев (технологии, институты): текстура
//! `textures/ui/bg.png` (звёздное небо). Исходник подгоняется под окно
//! режимом cover (центральная обрезка до пропорций окна + масштаб) —
//! иначе по бокам остаются полосы. Копия собирается заранее — при старте,
//! как только исходник догрузится, и далее только при изменении размера
//! окна; результат кешируется в `MenuBgCache` и в остальное время
//! рисуется как есть — пересборка картинки каждый кадр была бы
//! бессмысленной работой CPU, а первое открытие меню уже с готовым фоном.

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageSampler};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::ui::menus::MenuRoot;

/// Путь к исходной текстуре фона.
const BG_ASSET_PATH: &str = "textures/ui/bg.png";

/// Кеш фона: хэндл исходника и собранная под размер окна копия.
/// Переживает закрытие меню — повторное открытие рисует кеш сразу.
#[derive(Resource, Default)]
pub(crate) struct MenuBgCache {
    /// Хэндл исходной текстуры (грузится при старте; пока грузится —
    /// фон не показывается).
    pub(crate) source: Option<Handle<Image>>,
    /// Трансформированная копия под размер окна.
    pub(crate) scaled: Option<Handle<Image>>,
    /// Размер окна, под который собрана копия (логические px).
    pub(crate) scaled_size: Vec2,
}

/// Маркер узла фона внутри корня дерева (`MenuRoot`). Узел спавнится
/// первым ребёнком — картинка лежит под разделителями и карточками.
#[derive(Component)]
pub(crate) struct MenuBackground;

/// Однократная загрузка исходника фона при старте (вместе с данными деревьев).
pub(crate) fn load_bg_source(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(MenuBgCache {
        source: Some(assets.load(BG_ASSET_PATH)),
        ..default()
    });
}

/// Хэндл кешированного фона для показа (None — кеш ещё не собран).
pub(crate) fn cached_handle(cache: &MenuBgCache) -> Option<Handle<Image>> {
    cache.scaled.clone()
}

/// Поддерживает фон в актуальном состоянии: заранее собирает
/// трансформированную копию (при старте, как только исходник загрузится)
/// и пересобирает её при изменении размера окна — независимо от того,
/// открыто ли меню; узел фона открытого дерева получает кешированный
/// хэндл. Тяжёлая работа — только сборка копии в эти моменты.
pub(crate) fn update_menu_background(
    mut cache: ResMut<MenuBgCache>,
    mut images: ResMut<Assets<Image>>,
    roots: Query<(), With<MenuRoot>>,
    mut nodes: Query<&mut ImageNode, With<MenuBackground>>,
    windows: Query<&Window>,
) {
    let Some(source) = cache.source.clone() else {
        return;
    };
    let Some(window) = windows.iter().next() else {
        return;
    };
    let size = Vec2::new(window.width(), window.height());

    // Копия собирается заранее и далее только при смене размера окна;
    // иначе рисуется кешированная копия без пересборки.
    if cache.scaled_size != size || cache.scaled.is_none() {
        let Some(src) = images.get(&source) else {
            // Исходник ещё грузится — соберём в одном из следующих кадров.
            return;
        };
        match build_cover_image(src, size) {
            Some(scaled) => match &cache.scaled {
                // Копия уже существует: данные переписываются на месте,
                // хэндл (и ссылка в ImageNode) сохраняется.
                Some(handle) => {
                    if let Some(mut asset) = images.get_mut(handle) {
                        *asset = scaled;
                    }
                }
                None => cache.scaled = Some(images.add(scaled)),
            },
            None => {
                warn!("Фон меню: исходник {} не подошёл для трансформа", BG_ASSET_PATH);
                cache.source = None;
                return;
            }
        }
        cache.scaled_size = size;
    }

    // Узел фона есть только в открытом дереве (уходит вместе с корнем);
    // обычно он уже спавнится с кешированным хэндлом, но на всякий случай
    // (открытие в первый кадр работы игры) хэндл ставится и здесь.
    if roots.is_empty() {
        return;
    }
    if let Some(handle) = cache.scaled.clone() {
        for mut node in &mut nodes {
            if node.image != handle {
                node.image = handle.clone();
            }
        }
    }
}

/// Собирает копию исходника, накрывающую окно целиком (cover): центральная
/// обрезка до пропорций окна, затем масштаб до точного размера ближайшим
/// соседом (пиксель-арт). `None` — исходник ещё не готов или формат
/// пикселей неожиданный.
fn build_cover_image(source: &Image, window_size: Vec2) -> Option<Image> {
    let (w, h) = (window_size.x.max(1.0) as u32, window_size.y.max(1.0) as u32);
    let format = source.texture_descriptor.format;
    if format != TextureFormat::Rgba8UnormSrgb && format != TextureFormat::Rgba8Unorm {
        return None;
    }
    let data = source.data.as_ref()?;
    let src = image::RgbaImage::from_raw(source.width(), source.height(), data.clone())?;

    // Обрезка до пропорций окна: что лишнее по ширине или высоте — срежется
    // по центру, полос по бокам не останется.
    let (sw, sh) = (src.width(), src.height());
    let target_ratio = w as f32 / h as f32;
    let src_ratio = sw as f32 / sh as f32;
    let cropped = if src_ratio > target_ratio {
        let cw = (sh as f32 * target_ratio).max(1.0) as u32;
        image::imageops::crop_imm(&src, (sw - cw) / 2, 0, cw, sh).to_image()
    } else {
        let ch = (sw as f32 / target_ratio).max(1.0) as u32;
        image::imageops::crop_imm(&src, 0, (sh - ch) / 2, sw, ch).to_image()
    };
    let resized = image::imageops::resize(&cropped, w, h, image::imageops::FilterType::Nearest);

    let mut out = Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        resized.into_raw(),
        format,
        RenderAssetUsages::default(),
    );
    out.sampler = ImageSampler::nearest();
    Some(out)
}

#[cfg(test)]
#[path = "../tests/menu_bg.rs"]
mod tests;
