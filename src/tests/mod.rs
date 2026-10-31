//! Тесты на ключевые правила мира, разбитые по доменам. Тестовые модули,
//! которым нужен доступ к приватной реализации, подключаются из исходников
//! через `#[path]`, но их файлы также хранятся в этой папке.
//! Включаются только в `cargo test` (через `#[cfg(test)] mod tests` в main.rs).

mod armor;
mod build;
mod clothes;
mod climate;
mod crafts;
mod entities;
mod generation;
mod player;
mod rendering;
mod save;
mod time_scheduler;
mod vegetation;
mod world;
