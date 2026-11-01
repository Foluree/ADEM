//! Полосы потребностей захваченного существа в левом нижнем углу, снизу
//! вверх: здоровье (красная), голод (оранжевая), броня (серая). Каждая —
//! заливка на полупрозрачном фоне панели и число справа от полосы.
//!
//! Видны только в режиме захвата (игрока): в наблюдателе полосы не должны
//! занимать экран. У неразумного существа (волк) голода и брони нет — их
//! полосы скрываются, остаётся только здоровье. Максимум здоровья —
//! максимум вида захваченного существа. Значения читаются из сущности мира
//! каждый кадр — источник правды остаётся в симуляции.
//!
//! Здесь же кнопка-переключатель автономности захваченного существа
//! («ИИ», правый нижний угол): включённый ИИ передаёт существо общим
//! правилам мира (брожение животных, ИИ выживания людей), выключенный —
//! полный контроль контроллера.

use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::{Activate, ActivateOnPress, Button};

use crate::app::state::{GameMode, GameState};
use crate::simulation::armor::{ARMOR_RATING_MAX, rating as armor_rating};
use crate::simulation::entities::SimEntity;
use crate::simulation::hunger::HUNGER_MAX;
use crate::ui::UiFont;
use crate::ui::style;
use crate::ui::BlocksWorldInput;

pub struct HealthPlugin;
impl Plugin for HealthPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (spawn_health_bar, spawn_ai_toggle))
            .add_systems(
                Update,
                (
                    toggle_health_visibility,
                    refresh_health_bar,
                    toggle_ai_visibility,
                    refresh_ai_toggle,
                ),
            )
            .add_observer(on_ai_toggle_activated);
    }
}

/// Ширина и высота полосы (без рамки).
const BAR_WIDTH: f32 = 220.0;
const BAR_HEIGHT: f32 = 18.0;

/// Отступ от края экрана.
const MARGIN: f32 = 16.0;

/// Зазор между шкалами в колонке.
const BAR_GAP: f32 = 4.0;

/// Цвет заливки шкалы брони.
const ARMOR_COLOR: Color = Color::srgb_u8(0xCC, 0xCC, 0xCC);

/// Цвет заливки шкалы голода.
const HUNGER_COLOR: Color = Color::srgb_u8(0xFF, 0x88, 0x44);

/// Цвет заливки шкалы здоровья.
const HEALTH_COLOR: Color = Color::srgb(0.78, 0.16, 0.14);

/// Какая шкала: по маркеру заливка и текст находят своё значение игрока.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum BarKind {
    Armor,
    Hunger,
    Health,
}

/// Маркер корневой колонки (виден переключателю видимости).
#[derive(Component)]
struct HealthRoot;

/// Заливка шкалы: тип шкалы (цвет заливки уже стоит в `BackgroundColor`).
#[derive(Component)]
struct BarFill {
    kind: BarKind,
}

/// Число значения справа от полосы.
#[derive(Component)]
struct BarText {
    kind: BarKind,
}

/// Ряд полосы: виден/скрыт по виду захваченного существа (голод и броня
/// есть только у разумных).
#[derive(Component)]
struct BarRow {
    kind: BarKind,
}

fn spawn_health_bar(mut commands: Commands, font: Res<UiFont>) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(MARGIN),
                bottom: Val::Px(MARGIN),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(BAR_GAP),
                ..default()
            },
            HealthRoot,
        ))
        .with_children(|root| {
            // Снизу вверх: здоровье страдает от голода, голод — от брони
            // (броня поглощает урон, но не сытость). Броня сверху.
            spawn_bar(root, &font, BarKind::Health, HEALTH_COLOR);
            spawn_bar(root, &font, BarKind::Hunger, HUNGER_COLOR);
            spawn_bar(root, &font, BarKind::Armor, ARMOR_COLOR);
        });
}

/// Одна полоса: ряд из фона с рамкой, заливки на всю ширину и числа
/// справа. Значение заливку и текст обновляет `refresh_health_bar`.
fn spawn_bar(
    root: &mut ChildSpawnerCommands,
    font: &UiFont,
    kind: BarKind,
    fill_color: Color,
) {
    root.spawn((
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.0),
            ..default()
        },
        BarRow { kind },
    ))
    .with_children(|row| {
        // Пустая часть полосы — фон панели (полупрозрачный тёмный).
        row.spawn((
            Node {
                width: Val::Px(BAR_WIDTH),
                height: Val::Px(BAR_HEIGHT),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(style::background()),
            BorderColor::all(style::border()),
        ))
        .with_children(|bar| {
            bar.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(fill_color),
                BarFill { kind },
            ));
        });

        row.spawn((
            Text::new(String::new()),
            TextFont {
                font: font.0.clone().into(),
                font_size: 14.0.into(),
                ..default()
            },
            TextColor(style::text()),
            BarText { kind },
        ));
    });
}

fn toggle_health_visibility(
    state: Res<GameState>,
    mut roots: Query<&mut Node, With<HealthRoot>>,
) {
    let visible = state.mode == GameMode::Player;
    for mut node in &mut roots {
        let want = if visible { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
    }
}

/// Значение шкалы и её максимум по типу. Максимум здоровья — максимум
/// вида захваченного существа (волк 80, человек 100). Голод есть у всех
/// видов; броня вычисляется из надетых предметов (без «ума» — 0.0).
fn bar_value(kind: BarKind, entity: &SimEntity) -> (f32, f32) {
    match kind {
        BarKind::Armor => (
            entity.mind.as_ref().map_or(0.0, |m| armor_rating(&m.inventory)),
            ARMOR_RATING_MAX,
        ),
        BarKind::Hunger => (entity.hunger.value(), HUNGER_MAX),
        BarKind::Health => (entity.hp, entity.kind.max_hp()),
    }
}

/// Обновляет полосы: ширину заливки, число справа и видимость рядов
/// (броня есть только у разумных существ). Пишет только реальное
/// изменение, чтобы не дёргать переразметку каждый кадр.
fn refresh_health_bar(
    state: Res<GameState>,
    mut fills: Query<(&mut Node, &BarFill)>,
    mut texts: Query<(&BarText, &mut Text)>,
    mut rows: Query<(&BarRow, &mut Node), Without<BarFill>>,
) {
    if state.mode != GameMode::Player {
        return;
    }
    let Some(p) = state.controlled_entity() else {
        return;
    };

    // Броня существует только у существа с «умом»; у волка ряд скрывается
    // целиком. Голод у всех видов — волк тоже голодает.
    let sentient = p.mind.is_some();
    for (row, mut node) in &mut rows {
        let want = match row.kind {
            BarKind::Armor if !sentient => Display::None,
            _ => Display::Flex,
        };
        if node.display != want {
            node.display = want;
        }
    }

    for (mut node, fill) in &mut fills {
        let (value, max) = bar_value(fill.kind, p);
        let ratio = (value / max).clamp(0.0, 1.0);
        let want = Val::Percent(ratio * 100.0);
        if node.width != want {
            node.width = want;
        }
    }
    for (bar, mut text) in &mut texts {
        let (value, _) = bar_value(bar.kind, p);
        let want = format!("{value:.1}");
        if text.as_str() != want {
            **text = want.clone();
        }
    }
}

// --- Кнопка автономности захваченного существа («ИИ») ---

/// Маркер корня кнопки ИИ в правом нижнем углу (режим игрока).
#[derive(Component)]
struct AiToggleRoot;

/// Маркер самой кнопки: обсервер и стили работают только с ней, а не со
/// всеми `Button` мира (глобальный запрос воевал за фон/рамку с hotbar,
/// панелью и меню — мигание и «залипшие» обводки).
#[derive(Component)]
struct AiToggleButton;

/// Текст кнопки: «ИИ: вкл/выкл» по состоянию `GameState.controlled_ai`.
#[derive(Component)]
struct AiToggleLabel;

/// Кнопка «ИИ»: включает/выключает автономное поведение захваченного
/// существа. Выключенный ИИ (по умолчанию) — полный контроль контроллера;
/// включённый — существо живёт по общим правилам мира.
fn spawn_ai_toggle(mut commands: Commands, font: Res<UiFont>) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(MARGIN),
                bottom: Val::Px(MARGIN),
                ..default()
            },
            AiToggleRoot,
        ))
        .with_children(|root| {
            root.spawn((
                Button,
                Node {
                    height: Val::Px(28.0),
                    padding: UiRect::horizontal(Val::Px(10.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(style::surface()),
                BorderColor::all(style::border()),
                BlocksWorldInput,
                ActivateOnPress,
                AiToggleButton,
            ))
            .with_children(|button| {
                button.spawn((
                    Text::new(String::new()),
                    TextFont {
                        font: font.0.clone().into(),
                        font_size: 14.0.into(),
                        ..default()
                    },
                    TextColor(style::text()),
                    AiToggleLabel,
                ));
            });
        });
}

/// Клик по кнопке: переключение автономности. Активация приходит только
/// от помеченной кнопки; вне режима игрока кнопка скрыта, проверка —
/// страховка.
fn on_ai_toggle_activated(
    activate: On<Activate>,
    buttons: Query<(), With<AiToggleButton>>,
    mut state: ResMut<GameState>,
) {
    if !buttons.contains(activate.entity) || state.mode != GameMode::Player {
        return;
    }
    state.controlled_ai = !state.controlled_ai;
}

/// Кнопка видна только в режиме игрока, как полосы потребностей.
fn toggle_ai_visibility(state: Res<GameState>, mut roots: Query<&mut Node, With<AiToggleRoot>>) {
    for mut node in &mut roots {
        let want = if state.mode == GameMode::Player {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != want {
            node.display = want;
        }
    }
}

/// Подпись и фон кнопки по состоянию: включённый ИИ — акцентная рамка.
fn refresh_ai_toggle(
    state: Res<GameState>,
    mut labels: Query<&mut Text, With<AiToggleLabel>>,
    mut buttons: Query<
        (&mut BackgroundColor, &mut BorderColor, Has<Pressed>),
        (With<AiToggleButton>, With<Button>),
    >,
) {
    let want_text = if state.controlled_ai { "ИИ: вкл" } else { "ИИ: выкл" };
    for text in &mut labels {
        crate::ui::set_text_if_neq(text, want_text);
    }
    for (mut background, mut border, pressed) in &mut buttons {
        let want_bg = if pressed {
            style::surface_active()
        } else {
            style::surface()
        };
        background.set_if_neq(BackgroundColor(want_bg));
        let want_border = if state.controlled_ai {
            style::surface_active()
        } else {
            style::border()
        };
        border.set_if_neq(BorderColor::all(want_border));
    }
}
