//! Модальные меню технологий (T) и соц. институтов (Y): полноценные
//! полноэкранные деревья слева-направо. Деревья ветвятся: основная
//! последовательная цепочка задаёт эпохи, боковые ветви требуют узлы
//! цепочки и раскладываются в те же колонки глубины; границы эпох
//! отмечены вертикальными светло-серыми линиями (позиции
//! разделителей — константы в этом файле). Корни (без требований) слева,
//! ветви растут вправо по колонкам глубины; соединители — гладкие
//! кубические Безье без изломов (полилиния из повёрнутых через
//! `UiTransform` сегментов, приём из бестиария Endless_Space).
//! Обзор: зум колёсиком к курсору, панорама зажатой ПКМ. Вид не
//! сохраняется: ресурс создаётся при открытии (дерево показывается
//! целиком) и удаляется при закрытии любым путём.
//! Данные грузятся из `data/science/tech_*.json` (технологии по эпохам)
//! и `data/culture/social_*.json` (институты по эпохам) один раз при старте.
//! Фон меню — текстура `textures/ui/bg.png`, см. `ui/menu_bg.rs`.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use bevy::input::mouse::MouseWheel;
use bevy::input_focus::InputFocus;
use bevy::math::Rot2;
use bevy::prelude::*;
use bevy::ui::UiTransform;
use bevy::ui_widgets::TextInput;
use serde::Deserialize;

use crate::app::settings::{ControlAction, Settings};
use crate::app::state::{GameState, MenuKind};
use crate::ui::menu_bg::{self, MenuBackground, MenuBgCache};
use crate::ui::pause_menu::BindingCapture;
use crate::ui::{style, text_input_focused, BlocksWorldInput};
use crate::ui::UiFont;

pub struct MenusPlugin;

impl Plugin for MenusPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TechData>()
            .init_resource::<SocialData>()
            .init_resource::<MenuBgCache>()
            .add_systems(Startup, (load_data, menu_bg::load_bg_source))
            .add_systems(
                Update,
                (toggle_menus, cleanup_tree_view, pan_zoom_tree, menu_bg::update_menu_background).chain(),
            );
    }
}

// ---------- Данные ----------

#[derive(Deserialize, Default, Resource, Debug)]
pub struct TechData {
    #[serde(default)]
    pub technologies: Vec<TechEntry>,
}

#[allow(unused)]
#[derive(Deserialize, Default, Debug, Clone)]
pub struct TechEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub era: String,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub description: String,
}

#[derive(Deserialize, Default, Resource, Debug)]
pub struct SocialData {
    #[serde(default)]
    pub institutions: Vec<SocialEntry>,
}

#[allow(unused)]
#[derive(Deserialize, Default, Debug, Clone)]
pub struct SocialEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub era: String,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub description: String,
}

// ---------- Компоненты и константы холста ----------

/// Маркер корня техно/социального дерева (и творческого меню). Виден
/// пауз-меню: их ESC-цепочка закрывает эти меню тем же нажатием.
#[derive(Component)]
pub(crate) struct MenuRoot;

/// Сторона карточки: ширина и высота (px холста, масштабируются зумом).
const CARD_W: f32 = 240.0;
const CARD_H: f32 = 110.0;
/// Горизонтальный зазор между колонками и вертикальный между карточками.
const GAP_X: f32 = 130.0;
const GAP_Y: f32 = 44.0;
/// Базовая толщина линий, px холста; на экране клампится в [MIN, MAX].
const LINE_PX: f32 = 3.0;
const LINE_MIN_PX: f32 = 1.0;
const LINE_MAX_PX: f32 = 8.0;
/// Сколько сегментов на одно Безье-ребро (10 хватает для гладкости на
/// максимальном зуме; каждый сегмент — отдельная UI-сущность, которых
/// в дереве тысячи).
const CURVE_SEGMENTS: usize = 10;
/// Пределы зума.
const MIN_ZOOM: f32 = 0.2;
const MAX_ZOOM: f32 = 2.5;
/// Ниже этих зумов текст карточки прячется: нечитаемый обрезанный текст
/// только шумит на обзоре (описание раньше эпохи, эпоха раньше названия).
const DESC_MIN_ZOOM: f32 = 0.5;
const ERA_MIN_ZOOM: f32 = 0.35;
const TITLE_MIN_ZOOM: f32 = 0.25;

/// Цвет линий-соединителей.
const LINE_COLOR: Color = Color::srgba(0.52, 0.57, 0.70, 0.85);

/// Цвет вертикальных линий-разделителей эпох (светло-серый).
const ERA_LINE_COLOR: Color = Color::srgba(0.72, 0.73, 0.76, 0.45);

/// Толщина линии-разделителя эпох, px холста.
const ERA_LINE_PX: f32 = 2.0;

/// Отступ разделителя эпох выше и ниже дерева, px холста.
const ERA_LINE_MARGIN: f32 = 60.0;

/// Сколько колонок (последовательных карточек) занимает каждая эпоха в
/// дереве технологий — порядок эпох: каменный, бронзовый, железный век,
/// средневековье. Разделители рисуются после каждой границы, кроме
/// последней. Значение правится вместе с данными `data/science/`.
const TECH_ERA_COLUMNS: [usize; 4] = [10, 10, 10, 10];

/// То же для дерева социальных институтов (`data/culture/`).
const SOCIAL_ERA_COLUMNS: [usize; 4] = [8, 8, 8, 8];

/// Позиционируемый узел холста: базовая (немасштабированная) геометрия.
/// Экранная позиция/размер = `offset + base * zoom` (как в бестиарии).
#[derive(Component)]
struct CanvasNode {
    base: Vec2,
    size: Vec2,
}

/// Один сегмент кривой (короткий прямоугольник между точками полилинии
/// Безье). Позицию, длину, толщину и доворот каждый кадр считает обзор.
#[derive(Component)]
struct CurveSegment {
    a: Vec2,
    b: Vec2,
}

/// Текст карточки с базовым размером шрифта (масштабируется зумом).
#[derive(Component)]
struct CardTitle {
    base: f32,
}
#[derive(Component)]
struct CardEra {
    base: f32,
}
#[derive(Component)]
struct CardDesc {
    base: f32,
}

/// Вид дерева: сдвиг (экранные px) и зум. Живёт только пока дерево открыто.
#[derive(Resource, Debug)]
struct TreeView {
    offset: Vec2,
    zoom: f32,
}

// ---------- Загрузка ----------

/// Читает каталоги данных эпох: технологии — `data/science/tech_*.json`,
/// институты — `data/culture/social_*.json`. Файлы эпох сливаются в
/// порядке лексикографии имён (детерминизм, как у рецептов крафта);
/// ошибки отдельных файлов не роняют игру — только лог. После загрузки
/// данные проверяются на типовые ошибки разметки (см. `validate_tree`).
fn load_data(mut tech: ResMut<TechData>, mut social: ResMut<SocialData>) {
    *tech = load_tech_dir("data/science");
    *social = load_social_dir("data/culture");
    validate_tree("технологии", &tech.technologies);
    validate_tree("институты", &social.institutions);
}

/// Доступ узлов разных типов (технологии, институты) к полям проверки.
trait TreeNode {
    fn node_id(&self) -> &str;
    fn node_requires(&self) -> &[String];
}

impl TreeNode for TechEntry {
    fn node_id(&self) -> &str {
        &self.id
    }
    fn node_requires(&self) -> &[String] {
        &self.requires
    }
}

impl TreeNode for SocialEntry {
    fn node_id(&self) -> &str {
        &self.id
    }
    fn node_requires(&self) -> &[String] {
        &self.requires
    }
}

/// Проверяет данные дерева и печатает проблемы в лог (загрузка не роняет —
/// раскладка переживает и дубликаты, и неизвестные требования, и циклы,
/// но молча висящая ветка или потерянный узел — ловушка при правке JSON).
/// Проверки: уникальные id, требования к известным узлам, сходимость
/// глубин требований (несходимость = цикл, включая самоссылку).
/// Возвращает число найденных проблем (для тестов).
fn validate_tree(kind: &str, entries: &[impl TreeNode]) -> usize {
    let mut problems = 0;
    let mut index: HashMap<&str, usize> = HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        if index.insert(entry.node_id(), i).is_some() {
            warn!(
                "Данные деревьев ({}): узел «{}» объявлен больше одного раза",
                kind,
                entry.node_id()
            );
            problems += 1;
        }
    }

    // Глубины — та же итеративная раскладка, что и у дерева: без циклов
    // она сходится, с циклом глубины растут до конца бюджета проходов и
    // хотя бы одно ребро остаётся неудовлетворённым.
    let mut depth = vec![0_usize; entries.len()];
    for _ in 0..entries.len() {
        let mut changed = false;
        for (i, entry) in entries.iter().enumerate() {
            for req in entry.node_requires() {
                if let Some(&j) = index.get(req.as_str())
                    && depth[i] < depth[j] + 1
                {
                    depth[i] = depth[j] + 1;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    let mut warned_cycles = HashSet::new();
    let mut warned_unknown = HashSet::new();
    for (i, entry) in entries.iter().enumerate() {
        for req in entry.node_requires() {
            match index.get(req.as_str()) {
                None => {
                    if warned_unknown.insert((entry.node_id(), req.as_str())) {
                        warn!(
                            "Данные деревьев ({}): «{}» требует неизвестный узел «{}»",
                            kind,
                            entry.node_id(),
                            req
                        );
                        problems += 1;
                    }
                }
                Some(&j) if depth[i] < depth[j] + 1 && warned_cycles.insert(entry.node_id()) => {
                    warn!(
                        "Данные деревьев ({}): требования не сходятся — в дереве цикл (первый несходящийся узел «{}»)",
                        kind,
                        entry.node_id()
                    );
                    problems += 1;
                }
                _ => {}
            }
        }
    }
    if problems > 0 {
        warn!("Данные деревьев ({}): всего проблем: {}", kind, problems);
    }
    problems
}

/// Список `*.json`-файлов каталога в лексикографическом порядке.
/// Ошибка чтения каталога — пустой список (нет данных, нет дерева).
fn json_files(dir: &str) -> Vec<String> {
    match fs::read_dir(dir) {
        Ok(entries) => {
            let mut paths: Vec<String> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "json"))
                .filter_map(|p| p.to_str().map(str::to_string))
                .collect();
            paths.sort();
            paths
        }
        Err(e) => {
            warn!("Данные деревьев: каталог {dir} не прочитан ({e})");
            Vec::new()
        }
    }
}

/// Загружает все файлы технологий эпохи и сливает их в один набор.
fn load_tech_dir(dir: &str) -> TechData {
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

/// Загружает все файлы институтов эпохи и сливает их в один набор.
fn load_social_dir(dir: &str) -> SocialData {
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

/// Читает и десериализует JSON-файл.
fn read_json<T: for<'de> Deserialize<'de>>(path: &str) -> Result<T, String> {
    let p = Path::new(path);
    let text = fs::read_to_string(p).map_err(|e| format!("{path}: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))
}

// ---------- Управление меню ----------

/// Обрабатывает горячие клавиши меню (бинды из настроек). Пока фокус
/// в текстовом поле, идёт захват бинда или открыт пауз-меню,
/// хоткеи не срабатывают.
fn toggle_menus(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<GameState>,
    settings: Res<Settings>,
    capture: Res<BindingCapture>,
    tech: Res<TechData>,
    social: Res<SocialData>,
    bg: Res<MenuBgCache>,
    font: Res<UiFont>,
    windows: Query<&Window>,
    existing: Query<Entity, With<MenuRoot>>,
    focus: Res<InputFocus>,
    text_inputs: Query<(), With<TextInput>>,
) {
    if text_input_focused(&focus, &text_inputs) || capture.0.is_some() {
        return;
    }
    if state.active_menu == Some(MenuKind::Pause) {
        return;
    }
    let want_tech = keys.just_pressed(settings.controls.key(ControlAction::TechMenu));
    let want_social = keys.just_pressed(settings.controls.key(ControlAction::SocialMenu));
    if !want_tech && !want_social {
        return;
    }
    let target = if want_tech { MenuKind::Tech } else { MenuKind::Social };

    let mut already_open = false;
    for e in &existing {
        commands.entity(e).despawn();
        already_open = true;
    }
    if already_open && state.active_menu == Some(target) {
        state.active_menu = None;
        return;
    }

    state.active_menu = Some(target);
    spawn_tree(&mut commands, target, &tech, &social, &bg, &font, &windows);
}

/// Убирает вид дерева, когда корень меню закрыт любым путём (клавиша,
/// ESC, переключение на творческое меню). Вид нарочно не переживает
/// закрытие: при следующем открытии дерево показывается целиком.
fn cleanup_tree_view(mut commands: Commands, roots: Query<(), With<MenuRoot>>) {
    if roots.is_empty() {
        commands.remove_resource::<TreeView>();
    }
}

// ---------- Раскладка дерева ----------

/// Запись дерева для раскладки: требования и тексты карточки.
struct TreeEntry {
    id: String,
    name: String,
    era: String,
    requires: Vec<String>,
    description: String,
}

/// Раскладка холста: позиции карточек (левый верхний угол), рёбра и
/// границы содержимого (для начального вида «дерево целиком»).
struct TreeLayout {
    cards: Vec<Vec2>,
    edges: Vec<(Vec2, Vec2)>,
    bounds_min: Vec2,
    bounds_max: Vec2,
}

/// Метка эры для карточки. Неизвестные эры показываются как есть:
/// новые значения в JSON не должны оставаться без подписи.
fn era_label(era: &str) -> String {
    match era {
        "stone_age" => "Каменный век".to_string(),
        "bronze_age" => "Бронзовый век".to_string(),
        "iron_age" => "Железный век".to_string(),
        "medieval" => "Средневековье".to_string(),
        other => other.to_string(),
    }
}

/// X-координаты вертикальных разделителей эпох (px холста) по числу
/// колонок каждой эпохи: линия ставится посередине зазора на границе
/// эпох; после последней границы линия не нужна.
fn era_separator_positions(era_columns: &[usize]) -> Vec<f32> {
    let pitch = CARD_W + GAP_X;
    let mut positions = Vec::new();
    let mut columns_passed = 0_usize;
    for (i, count) in era_columns.iter().enumerate() {
        columns_passed += count;
        if i + 1 < era_columns.len() {
            positions.push(columns_passed as f32 * pitch - GAP_X * 0.5);
        }
    }
    positions
}

/// Глубина узла = длина самого длинного пути от корня. Считается
/// итеративно до стабилизации: цикл в данных (битый JSON) не подвешивает
/// раскладку, глубина просто перестаёт расти.
fn compute_depths(entries: &[TreeEntry]) -> Vec<usize> {
    let index: HashMap<&str, usize> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| (e.id.as_str(), i))
        .collect();
    let mut depth = vec![0_usize; entries.len()];
    for _ in 0..entries.len() {
        let mut changed = false;
        for (i, entry) in entries.iter().enumerate() {
            for req in &entry.requires {
                if let Some(&j) = index.get(req.as_str())
                    && depth[i] < depth[j] + 1
                {
                    depth[i] = depth[j] + 1;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    depth
}

/// Средний Y размещённых родителей записи (0.0 — корни без родителей).
/// Ключ сортировки колонки: дети выстраиваются примерно под своими
/// родителями, линии меньше перекрещиваются.
fn parent_avg_y(
    center_y: &[Option<f32>],
    index: &HashMap<&str, usize>,
    entry: &TreeEntry,
) -> f32 {
    let ys: Vec<f32> = entry
        .requires
        .iter()
        .filter_map(|r| index.get(r.as_str()).copied())
        .filter_map(|j| center_y[j])
        .collect();
    if ys.is_empty() {
        0.0
    } else {
        ys.iter().sum::<f32>() / ys.len() as f32
    }
}

/// Раскладка слева-направо: X по колонкам глубины, Y — центрированный
/// вертикальный стек колонки. Внутри колонки карточки сортируются по
/// среднему Y своих родителей (уже размещённых слева) — линии меньше
/// перекрещиваются. Данные маленькие, сложная маршрутизация не нужна.
fn compute_tree_layout(entries: &[TreeEntry]) -> TreeLayout {
    let depth = compute_depths(entries);
    let max_depth = depth.iter().copied().max().unwrap_or(0);
    let pitch_x = CARD_W + GAP_X;
    let index: HashMap<&str, usize> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| (e.id.as_str(), i))
        .collect();

    let mut center_y: Vec<Option<f32>> = vec![None; entries.len()];
    for column in 0..=max_depth {
        let mut order: Vec<usize> = (0..entries.len())
            .filter(|&i| depth[i] == column)
            .collect();
        order.sort_by(|&a, &b| {
            let ya = parent_avg_y(&center_y, &index, &entries[a]);
            let yb = parent_avg_y(&center_y, &index, &entries[b]);
            ya.total_cmp(&yb).then(a.cmp(&b))
        });

        let total_h = order.len() as f32 * CARD_H + (order.len().saturating_sub(1)) as f32 * GAP_Y;
        let mut y = -total_h * 0.5 + CARD_H * 0.5;
        for &i in &order {
            center_y[i] = Some(y);
            y += CARD_H + GAP_Y;
        }
    }

    let mut cards = Vec::with_capacity(entries.len());
    let mut bounds_min = Vec2::splat(f32::MAX);
    let mut bounds_max = Vec2::splat(f32::MIN);
    for (i, _) in entries.iter().enumerate() {
        let pos = Vec2::new(
            depth[i] as f32 * pitch_x,
            center_y[i].unwrap_or(0.0) - CARD_H * 0.5,
        );
        bounds_min = bounds_min.min(pos);
        bounds_max = bounds_max.max(pos + Vec2::new(CARD_W, CARD_H));
        cards.push(pos);
    }

    let mut edges = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        let child_center = cards[i] + Vec2::new(CARD_W * 0.5, CARD_H * 0.5);
        let end = Vec2::new(child_center.x - CARD_W * 0.5, child_center.y);
        for req in &entry.requires {
            let Some(&j) = index.get(req.as_str()) else {
                continue;
            };
            let parent_center = cards[j] + Vec2::new(CARD_W * 0.5, CARD_H * 0.5);
            let start = Vec2::new(parent_center.x + CARD_W * 0.5, parent_center.y);
            edges.push((start, end));
        }
    }

    TreeLayout {
        cards,
        edges,
        bounds_min,
        bounds_max,
    }
}

// ---------- Отрисовка ----------

/// Создаёт полноэкранное дерево. Пустые данные — экран с сообщением,
/// остальная механика (ESC, зум) работает как обычно.
fn spawn_tree(
    commands: &mut Commands,
    kind: MenuKind,
    tech: &TechData,
    social: &SocialData,
    bg: &MenuBgCache,
    font: &UiFont,
    windows: &Query<&Window>,
) {
    let (title, entries) = match kind {
        MenuKind::Tech => (
            "Дерево технологий",
            tech.technologies
                .iter()
                .map(|t| TreeEntry {
                    id: t.id.clone(),
                    name: t.name.clone(),
                    era: t.era.clone(),
                    requires: t.requires.clone(),
                    description: t.description.clone(),
                })
                .collect::<Vec<_>>(),
        ),
        MenuKind::Social => (
            "Социальные институты",
            social
                .institutions
                .iter()
                .map(|s| TreeEntry {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    era: s.era.clone(),
                    requires: s.requires.clone(),
                    description: s.description.clone(),
                })
                .collect::<Vec<_>>(),
        ),
        // Пауз-меню рисуется отдельно, творческое — в ui/creative.rs.
        _ => return,
    };

    let layout = compute_tree_layout(&entries);
    // Число колонок эпох — своё у каждого дерева (правится в константах).
    let era_columns: &[usize] = match kind {
        MenuKind::Tech => &TECH_ERA_COLUMNS,
        MenuKind::Social => &SOCIAL_ERA_COLUMNS,
        _ => return,
    };

    // Стартовый вид: дерево целиком по центру, зум вписывает его в экран
    // (крупные деревья не должны вылезать, маленькие не растягиваются).
    let (w, h) = windows
        .iter()
        .next()
        .map(|win| (win.width(), win.height()))
        .unwrap_or((1280.0, 720.0));
    let content = (layout.bounds_max - layout.bounds_min).max(Vec2::ONE);
    let zoom = ((w * 0.85) / content.x)
        .min((h * 0.80) / content.y)
        .clamp(MIN_ZOOM, 1.0);
    let content_center = (layout.bounds_min + layout.bounds_max) * 0.5;
    commands.insert_resource(TreeView {
        offset: Vec2::new(w, h) * 0.5 - content_center * zoom,
        zoom,
    });

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            // Глухой тёмный фон: полноэкранное меню полностью закрывает
            // собой мир и остальные элементы UI.
            BackgroundColor(Color::srgb(0.09, 0.10, 0.12)),
            MenuRoot,
            BlocksWorldInput,
        ))
        .with_children(|root| {
            // Фон — первым ребёнком: картинка лежит под разделителями,
            // линиями и карточками. Кеш обычно собран заранее (см.
            // `menu_bg.rs`), но пока исходник грузится узел рисует пустой
            // `ImageNode` — хэндл поставит `update_menu_background`, до тех
            // пор остаётся глухой тёмный цвет корня.
            root.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                ImageNode {
                    image: menu_bg::cached_handle(bg).unwrap_or_default(),
                    ..default()
                },
                MenuBackground,
            ));
            // Разделители эпох — сразу после фона: лежат под линиями и карточками.
            // Вертикальные светло-серые линии на всю высоту дерева; масштаб
            // панорамой считает общий обход CanvasNode.
            let top = layout.bounds_min.y - ERA_LINE_MARGIN;
            let height = (layout.bounds_max.y - layout.bounds_min.y) + ERA_LINE_MARGIN * 2.0;
            for x in era_separator_positions(era_columns) {
                root.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(x),
                        top: Val::Px(top),
                        width: Val::Px(ERA_LINE_PX),
                        height: Val::Px(height),
                        ..default()
                    },
                    BackgroundColor(ERA_LINE_COLOR),
                    CanvasNode {
                        base: Vec2::new(x, top),
                        size: Vec2::new(ERA_LINE_PX, height),
                    },
                ));
            }
            // Сначала линии (лежат под карточками), затем карточки.
            for (a, b) in &layout.edges {
                spawn_edge(root, *a, *b);
            }
            for (i, pos) in layout.cards.iter().enumerate() {
                let entry = &entries[i];
                spawn_card(root, *pos, entry, font);
            }
            // Шапка и подсказка поверх.
            root.spawn((
                Text::new(title),
                TextFont {
                    font: font.0.clone().into(),
                    font_size: 26.0.into(),
                    ..default()
                },
                TextColor(style::text()),
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(12.0),
                    left: Val::Px(16.0),
                    ..default()
                },
            ));
            root.spawn((
                Text::new("ПКМ — панорама, колесо — зум, Esc — закрыть"),
                TextFont {
                    font: font.0.clone().into(),
                    font_size: 13.0.into(),
                    ..default()
                },
                TextColor(style::text_dim()),
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(46.0),
                    left: Val::Px(16.0),
                    ..default()
                },
            ));
            if entries.is_empty() {
                root.spawn((
                    Text::new("(данные не загружены)"),
                    TextFont {
                        font: font.0.clone().into(),
                        font_size: 16.0.into(),
                        ..default()
                    },
                    TextColor(style::text_dim()),
                    Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(80.0),
                        left: Val::Px(16.0),
                        ..default()
                    },
                ));
            }
        });
}

/// Один прямой отрезок кривой: позиция/длина/доворот ставятся обзором
/// каждый кадр, здесь — только стартовые значения.
fn spawn_edge(parent: &mut ChildSpawnerCommands, a: Vec2, b: Vec2) {
    let pts = bezier_path(a, b, CURVE_SEGMENTS);
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(a.x),
                top: Val::Px(a.y),
                width: Val::Px(1.0),
                height: Val::Px(LINE_PX),
                ..default()
            },
            BackgroundColor(LINE_COLOR),
            CurveSegment { a, b },
            UiTransform::default(),
        ));
    }
}

/// Точки кубической Безье от правого края родителя (`a`) до левого края
/// ребёнка (`b`). Обе касательные горизонтальные — гладкий S-коннектор
/// без изломов: кривая выходит из родителя и входит в ребёнка по
/// касательной. Почти вертикальные пары — прямым отрезком.
fn bezier_path(a: Vec2, b: Vec2, n: usize) -> Vec<Vec2> {
    if (b.x - a.x).abs() < 0.5 {
        return vec![a, b];
    }
    let dx = (b.x - a.x) * 0.55;
    let (p0, p1, p2, p3) = (
        a,
        Vec2::new(a.x + dx, a.y),
        Vec2::new(b.x - dx, b.y),
        b,
    );
    let mut pts = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let mt = 1.0 - t;
        let x = mt * mt * mt * p0.x
            + 3.0 * mt * mt * t * p1.x
            + 3.0 * mt * t * t * p2.x
            + t * t * t * p3.x;
        let y = mt * mt * mt * p0.y
            + 3.0 * mt * mt * t * p1.y
            + 3.0 * mt * t * t * p2.y
            + t * t * t * p3.y;
        pts.push(Vec2::new(x, y));
    }
    pts
}

/// Карточка дерева: название, эра, описание. Текст обрезан границами
/// карточки (`overflow: hidden`) — на малом зуме строки не вылезают на
/// соседей.
fn spawn_card(
    parent: &mut ChildSpawnerCommands,
    top_left: Vec2,
    entry: &TreeEntry,
    font: &UiFont,
) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(top_left.x),
                top: Val::Px(top_left.y),
                width: Val::Px(CARD_W),
                height: Val::Px(CARD_H),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(10.0)),
                row_gap: Val::Px(4.0),
                border_radius: BorderRadius::all(Val::Px(8.0)),
                overflow: Overflow::hidden(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.16, 0.17, 0.20, 0.95)),
            BorderColor::all(style::border()),
            CanvasNode {
                base: top_left,
                size: Vec2::new(CARD_W, CARD_H),
            },
        ))
        .with_children(|card| {
            card.spawn((
                Text::new(entry.name.clone()),
                TextFont {
                    font: font.0.clone().into(),
                    font_size: 17.0.into(),
                    ..default()
                },
                TextColor(style::text()),
                CardTitle { base: 17.0 },
            ));
            card.spawn((
                Text::new(era_label(&entry.era)),
                TextFont {
                    font: font.0.clone().into(),
                    font_size: 12.0.into(),
                    ..default()
                },
                TextColor(style::text_dim()),
                CardEra { base: 12.0 },
            ));
            if !entry.description.is_empty() {
                card.spawn((
                    Text::new(entry.description.clone()),
                    TextFont {
                        font: font.0.clone().into(),
                        font_size: 12.0.into(),
                        ..default()
                    },
                    TextColor(style::text_dim()),
                    CardDesc { base: 12.0 },
                ));
            }
        });
}

/// Панорама (зажатая ПКМ) + зум (колесо к курсору) + раскладка узлов.
/// Содержимое карточек масштабируется только при смене зума: смена
/// шрифта каждый кадр дёргала бы переразметку (приём из бестиария).
fn pan_zoom_tree(
    view: Option<ResMut<TreeView>>,
    windows: Query<&Window>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<MouseWheel>,
    mut prev_cursor: Local<Option<Vec2>>,
    mut last_zoom: Local<f32>,
    mut last_offset: Local<Vec2>,
    mut cards: Query<(&CanvasNode, &mut Node), Without<CurveSegment>>,
    mut segments: Query<(&CurveSegment, &mut Node, &mut UiTransform)>,
    mut titles: Query<(&CardTitle, &mut TextFont, &mut Visibility), (Without<CardEra>, Without<CardDesc>)>,
    mut eras: Query<(&CardEra, &mut TextFont, &mut Visibility), (Without<CardTitle>, Without<CardDesc>)>,
    mut descs: Query<(&CardDesc, &mut TextFont, &mut Visibility), (Without<CardTitle>, Without<CardEra>)>,
) {
    let Some(mut view) = view else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };

    // Панорама зажатой ПКМ.
    if mouse.pressed(MouseButton::Right) {
        if let Some(cursor) = window.cursor_position() {
            if let Some(prev) = *prev_cursor {
                view.offset += cursor - prev;
            }
            *prev_cursor = Some(cursor);
        }
    } else {
        *prev_cursor = None;
    }

    // Зум колесом к курсору.
    let mut scroll = 0.0;
    for e in wheel.read() {
        scroll += e.y;
    }
    if scroll != 0.0
        && let Some(cursor) = window.cursor_position()
    {
        let old = view.zoom;
        let new = (old * 1.12_f32.powf(scroll)).clamp(MIN_ZOOM, MAX_ZOOM);
        view.offset = cursor - (cursor - view.offset) * (new / old);
        view.zoom = new;
    }

    let z = view.zoom;
    let o = view.offset;
    // Раскладка карточек и линий переписывается только при сдвиге вида
    // (или на входе в меню): мутации Node каждый кадр впустую грязнят
    // тысячи узлов и заставляют UI пересчитывать раскладку без нужды.
    let force = view.is_added();
    let view_changed = force || *last_offset != o || (z - *last_zoom).abs() > f32::EPSILON;
    if view_changed {
        *last_offset = o;
        for (canvas, mut node) in &mut cards {
            node.left = Val::Px(o.x + canvas.base.x * z);
            node.top = Val::Px(o.y + canvas.base.y * z);
            node.width = Val::Px(canvas.size.x * z);
            node.height = Val::Px(canvas.size.y * z);
            // Радиус и паддинги — в px холста: масштабируются зумом, иначе
            // на отдалении карточка «круглится», а отступы съедают текст.
            node.border_radius = BorderRadius::all(Val::Px(8.0 * z));
            node.padding = UiRect::all(Val::Px(10.0 * z));
            node.row_gap = Val::Px(4.0 * z);
        }

        // Линии: длина — честно зумом, толщина — кламп экранных пикселей.
        // Сегмент кладётся серединой на середину хорды и доворачивается
        // через `UiTransform` (поворот вокруг центра ноды).
        let line_px = (LINE_PX * z).clamp(LINE_MIN_PX, LINE_MAX_PX);
        for (seg, mut node, mut tr) in &mut segments {
            let a = o + seg.a * z;
            let b = o + seg.b * z;
            let d = b - a;
            let len = d.length().max(0.01);
            let mid = (a + b) * 0.5;
            node.left = Val::Px(mid.x - len * 0.5);
            node.top = Val::Px(mid.y - line_px * 0.5);
            node.width = Val::Px(len);
            node.height = Val::Px(line_px);
            tr.rotation = Rot2::radians(d.y.atan2(d.x));
        }
    }

    // Шрифты и пороги видимости — только при смене зума. `force` — вход
    // в меню: карточки заспавнены в базовом размере, а `last_zoom` мог
    // уцелеть с прошлого открытия с тем же значением.
    if force || (z - *last_zoom).abs() > f32::EPSILON {
        *last_zoom = z;
        let show_title = z >= TITLE_MIN_ZOOM;
        for (title, mut font, mut vis) in &mut titles {
            font.font_size = (title.base * z).clamp(6.0, 40.0).into();
            let want = if show_title {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            if *vis != want {
                *vis = want;
            }
        }
        let show_era = z >= ERA_MIN_ZOOM;
        for (era, mut font, mut vis) in &mut eras {
            font.font_size = (era.base * z).clamp(5.0, 28.0).into();
            let want = if show_era {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            if *vis != want {
                *vis = want;
            }
        }
        let show_desc = z >= DESC_MIN_ZOOM;
        for (desc, mut font, mut vis) in &mut descs {
            font.font_size = (desc.base * z).clamp(5.0, 28.0).into();
            let want = if show_desc {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            if *vis != want {
                *vis = want;
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/menus.rs"]
mod tests;
