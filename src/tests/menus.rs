use super::*;
use super::tech::{TechEntry, TECH_ERA_COLUMNS};
use super::tree_view::{CARD_W, GAP_X, era_separator_positions};

/// Узел для тестов валидации.
fn entry(id: &str, requires: &[&str]) -> TechEntry {
    TechEntry {
        id: id.to_string(),
        name: id.to_string(),
        era: "stone_age".to_string(),
        requires: requires.iter().map(|r| r.to_string()).collect(),
        description: String::new(),
    }
}

/// Разделители стоят посередине зазора между колонками эпох и не
/// ставятся после последней границы.
#[test]
fn era_separators_between_columns() {
    let pitch = CARD_W + GAP_X;

    // Три эпохи по 1 колонке: два разделителя — после 1-й и 2-й.
    assert_eq!(
        era_separator_positions(&[1, 1, 1]),
        vec![pitch - GAP_X * 0.5, 2.0 * pitch - GAP_X * 0.5]
    );

    // Реальная раскладка технологий: 10/10/10/10 колонок.
    let tech = era_separator_positions(&TECH_ERA_COLUMNS);
    assert_eq!(tech.len(), 3);
    assert_eq!(tech[0], 10.0 * pitch - GAP_X * 0.5);
    assert_eq!(tech[2], 30.0 * pitch - GAP_X * 0.5);

    // Одна эпоха — разделителей нет.
    assert!(era_separator_positions(&[12]).is_empty());
}

/// Чистые данные — ноль проблем.
#[test]
fn validation_accepts_clean_data() {
    let clean = vec![entry("a", &[]), entry("b", &["a"]), entry("c", &["a", "b"])];
    assert_eq!(validate_tree("тест", &clean), 0);
}

/// Неизвестное требование, дубликат и цикл — по одной проблеме; цикл
/// ловится и на самоссылке.
#[test]
fn validation_flags_problems() {
    let unknown = vec![entry("a", &["ghost"])];
    assert_eq!(validate_tree("тест", &unknown), 1);

    let duplicate = vec![entry("a", &[]), entry("a", &[])];
    assert_eq!(validate_tree("тест", &duplicate), 1);

    let cycle = vec![entry("a", &["b"]), entry("b", &["a"])];
    assert_eq!(validate_tree("тест", &cycle), 1);

    let self_reference = vec![entry("a", &["a"])];
    assert_eq!(validate_tree("тест", &self_reference), 1);

    // Длинный цикл тоже не ускользает: глубины не сходятся за бюджет.
    let long_cycle = vec![
        entry("a", &["d"]),
        entry("b", &["a"]),
        entry("c", &["b"]),
        entry("d", &["c"]),
    ];
    assert_eq!(validate_tree("тест", &long_cycle), 1);
}
