//! Поля мира справа на панели: размер мира и сид. Pending-текст живёт
//! в виджете до Enter; Escape откатывает ввод, без фокуса поле показывает
//! применённое значение. Применение перегенерирует мир целиком.

use bevy::input_focus::InputFocus;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextFilter, TextCursorStyle, TextEdit, TextLayout};
use bevy::ui_widgets::TextInput;

use crate::app::state::{ClockRes, GameState, SchedulerRes};
use crate::simulation::generation::generate_world;
use crate::simulation::scheduler::Scheduler;
use crate::simulation::vegetation;
use crate::simulation::world::WorldSize;
use crate::ui::UiFont;
use crate::ui::style;

use super::{column_node, row_node};

/// Маркер поля ввода размера мира.
#[derive(Component)]
pub(crate) struct WorldSizeInput;

/// Маркер поля ввода сида мира.
#[derive(Component)]
pub(crate) struct WorldSeedInput;

/// Ссылка на текст подписи сида мира.
#[derive(Component)]
pub(crate) struct WorldSeedLabel;

/// Ссылка на текст подписи размера мира (показывает применённое значение).
#[derive(Component)]
pub(crate) struct WorldSizeLabel;

/// Спавнит столбец полей «Мир» и «Сид» (вызывает `spawn_bottom_panel`).
pub(crate) fn spawn_world_fields(
    panel: &mut ChildSpawnerCommands,
    state: &GameState,
    font: &UiFont,
) {
    panel.spawn(column_node()).with_children(|col| {
        col.spawn(row_node()).with_children(|size| {
            size.spawn((
                Text::new(format!("Мир: {}", state.world_size.label())),
                TextFont {
                    font: font.0.clone().into(),
                    font_size: 14.0.into(),
                    ..default()
                },
                TextColor(style::text()),
                WorldSizeLabel,
            ));

            let mut editable = EditableText::new(state.world_size.tiles().to_string());
            editable.max_characters = Some(5);
            size.spawn((
                Node {
                    width: Val::Px(76.0),
                    padding: UiRect::all(Val::Px(7.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(style::surface()),
                BorderColor::from(style::border()),
                editable,
                EditableTextFilter::new(|c| c.is_ascii_digit()),
                TextLayout::no_wrap(),
                TextCursorStyle::default(),
                TabIndex(0),
                TextInput,
                TextFont {
                    font: font.0.clone().into(),
                    font_size: 14.0.into(),
                    ..default()
                },
                TextColor(style::text()),
                WorldSizeInput,
            ));
        });

        col.spawn(row_node()).with_children(|seed_row| {
            seed_row.spawn((
                Text::new(format!("Сид: {}", state.seed)),
                TextFont {
                    font: font.0.clone().into(),
                    font_size: 14.0.into(),
                    ..default()
                },
                TextColor(style::text()),
                WorldSeedLabel,
            ));

            let mut editable = EditableText::new(state.seed.to_string());
            editable.max_characters = Some(20);
            seed_row.spawn((
                Node {
                    width: Val::Px(120.0),
                    padding: UiRect::all(Val::Px(7.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(style::surface()),
                BorderColor::from(style::border()),
                editable,
                EditableTextFilter::new(|c| c.is_ascii_digit()),
                TextLayout::no_wrap(),
                TextCursorStyle::default(),
                TabIndex(0),
                TextInput,
                TextFont {
                    font: font.0.clone().into(),
                    font_size: 14.0.into(),
                    ..default()
                },
                TextColor(style::text()),
                WorldSeedInput,
            ));
        });
    });
}

/// Поле ввода размера мира: pending-текст живёт в виджете до применения.
///
/// Enter — применить (значение зажимается в лимиты мира и округляется до
/// кратности чанку, мир перегенерируется). Escape — отменить ввод и вернуть
/// применённое значение. Без фокуса поле всегда показывает применённый размер.
pub(crate) fn handle_world_size_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<GameState>,
    clock: Res<ClockRes>,
    mut sched: ResMut<SchedulerRes>,
    focus: Res<InputFocus>,
    mut inputs: Query<(Entity, &mut EditableText), With<WorldSizeInput>>,
    mut size_label: Query<&mut Text, With<WorldSizeLabel>>,
) {
    // Подпись отражает применённый размер: во время набора видно,
    // что pending в поле ещё не действует.
    if state.is_changed() {
        let want = format!("Мир: {}", state.world_size.label());
        for t in &mut size_label {
            crate::ui::set_text_if_neq(t, &want);
        }
    }

    let Ok((entity, mut editable)) = inputs.single_mut() else {
        return;
    };
    handle_world_field(
        &keys,
        &focus,
        entity,
        &mut editable,
        state.world_size.tiles(),
        |text| text.parse::<u32>().ok(),
        |side| {
            let size = WorldSize::new(side);
            if size != state.world_size {
                regenerate_world(size, state.seed, &mut state, &clock, &mut sched.0);
                state.world_size = size;
            }
            state.world_size.tiles().to_string()
        },
    );
}

/// Поле ввода сида мира: применяется по Enter.
pub(crate) fn handle_world_seed_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<GameState>,
    clock: Res<ClockRes>,
    mut sched: ResMut<SchedulerRes>,
    focus: Res<InputFocus>,
    mut inputs: Query<(Entity, &mut EditableText), With<WorldSeedInput>>,
    mut seed_label: Query<&mut Text, With<WorldSeedLabel>>,
) {
    if state.is_changed() {
        let want = format!("Сид: {}", state.seed);
        for t in &mut seed_label {
            crate::ui::set_text_if_neq(t, &want);
        }
    }

    let Ok((entity, mut editable)) = inputs.single_mut() else {
        return;
    };
    handle_world_field(
        &keys,
        &focus,
        entity,
        &mut editable,
        state.seed,
        |text| text.parse::<u64>().ok(),
        |seed| {
            if seed != state.seed {
                regenerate_world(
                    state.world_size,
                    seed,
                    &mut state,
                    &clock,
                    &mut sched.0,
                );
                state.seed = seed;
            }
            state.seed.to_string()
        },
    );
}

/// Общий каркас полей ввода «размер мира»/«сид»: pending-текст живёт в
/// виджете до Enter, Escape откатывает, без фокуса поле показывает
/// применённое значение. Разные у полей только тип значения (`parse`) и
/// `apply`, который применяет его и возвращает новый текст для поля.
fn handle_world_field<T>(
    keys: &ButtonInput<KeyCode>,
    focus: &InputFocus,
    entity: Entity,
    editable: &mut EditableText,
    committed: T,
    parse: fn(&str) -> Option<T>,
    apply: impl FnOnce(T) -> String,
) where
    T: ToString,
{
    let committed = committed.to_string();
    if focus.get() != Some(entity) {
        // Отменённый/неприменённый ввод не сохраняем.
        if editable.value().to_string() != committed {
            reset_editable(editable, &committed);
        }
        return;
    }

    if keys.just_pressed(KeyCode::Enter) {
        match parse(editable.value().to_string().trim()) {
            // Пустое поле или превышение разрядности откатывает ввод.
            Some(value) => {
                let applied = apply(value);
                reset_editable(editable, &applied);
            }
            None => reset_editable(editable, &committed),
        }
    } else if keys.just_pressed(KeyCode::Escape) {
        reset_editable(editable, &committed);
    }
}

/// Перегенерирует мир с новыми размером/сидом: генерация, посев цепочек
/// растительности, запрос перерисовки. Захваченное существо теряется:
/// мир заменён целиком, управлять не кем (можно захватить другое по F1).
/// Общая часть обоих полей ввода.
fn regenerate_world(
    size: WorldSize,
    seed: u64,
    state: &mut GameState,
    clock: &ClockRes,
    sched: &mut Scheduler,
) {
    let world = generate_world(size, seed);
    vegetation::seed_all(&world, sched, clock.0.tick);
    state.world = Some(world);
    state.pending_rerender = true;
    state.set_controlled(None);
}

/// Сбрасывает текстовое поле на значение: pending-правки стираются,
/// курсор — в конец.
pub(crate) fn reset_editable(editable: &mut EditableText, value: &str) {
    editable.pending_edits.clear();
    editable.pending_paste = None;
    editable.editor_mut().set_text(value);
    // Курсор в конец — то же, что делает EditableText::new.
    editable.queue_edit(TextEdit::TextEnd(false));
}
