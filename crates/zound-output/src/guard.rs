//! Защита от feedback-loop + структурированные ошибки команд.
//!
//! Защита: устройство, на котором сейчас идёт loopback-захват, нельзя ни
//! добавлять как output, ни запускать на нём тест-сигнал — иначе сигнал
//! отыграет в системе → попадёт в наш capture → отыграет снова →
//! бесконечная петля с возрастающей громкостью.
//!
//! Логика тривиальная (сравнение строк), но вынесена в отдельный модуль
//! из соображений тестируемости: реальный путь идёт через cpal-стримы,
//! ловить feedback вживую дорого. Здесь — pure-функции.
//!
//! Ошибки: фронт получает `CommandError` как структурированный JSON
//! (`{"kind":"feedbackBlocked","message":"..."}`) и matches по полю
//! `kind`, без парсинга строк.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Маркер ошибки feedback-loop. Дублирует `CommandError::FeedbackBlocked`,
/// оставлен для обратной совместимости с местами, которые работают с
/// `Result<_, String>` (engine ↔ Tauri-фасад).
pub const ERR_FEEDBACK_LOOP: &str = "feedback-default-blocked";

/// Структурированные ошибки команд. Frontend matches по полю `kind`,
/// текст в `message` — уже локализованный (через i18n) или
/// техническая деталь для логов.
///
/// Сериализуется в JSON в виде
/// `{"kind":"feedbackBlocked","message":"..."}`.
#[derive(Debug, Clone, Error, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CommandError {
    /// Попытка добавить устройство-источник loopback как output, или
    /// проиграть на нём тест-сигнал.
    #[error("feedback loop: {message}")]
    FeedbackBlocked { message: String },
    /// Запрошенное устройство не найдено (по имени или endpoint_id).
    #[error("device not found: {message}")]
    DeviceNotFound { message: String },
    /// Engine ещё не запущен (capture закрыт).
    #[error("engine not started: {message}")]
    EngineNotStarted { message: String },
    /// Audio-thread не отвечает (упал, channel disconnected).
    #[error("engine dead: {message}")]
    EngineDead { message: String },
    /// Тест-сигнал уже играет на устройстве.
    #[error("test already playing: {message}")]
    TestAlreadyPlaying { message: String },
    /// Запрос некорректен (неизвестный TestKind, off-range parameter, и т.п.).
    #[error("bad request: {message}")]
    BadRequest { message: String },
    /// Внутренняя ошибка backend-а / IO. Используется как catch-all.
    #[error("backend error: {message}")]
    Backend { message: String },
}

impl CommandError {
    pub fn feedback_blocked(msg: impl Into<String>) -> Self {
        Self::FeedbackBlocked {
            message: msg.into(),
        }
    }
    pub fn device_not_found(msg: impl Into<String>) -> Self {
        Self::DeviceNotFound {
            message: msg.into(),
        }
    }
    pub fn engine_not_started(msg: impl Into<String>) -> Self {
        Self::EngineNotStarted {
            message: msg.into(),
        }
    }
    pub fn engine_dead(msg: impl Into<String>) -> Self {
        Self::EngineDead {
            message: msg.into(),
        }
    }
    pub fn test_already_playing(msg: impl Into<String>) -> Self {
        Self::TestAlreadyPlaying {
            message: msg.into(),
        }
    }
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest {
            message: msg.into(),
        }
    }
    pub fn backend(msg: impl Into<String>) -> Self {
        Self::Backend {
            message: msg.into(),
        }
    }

    /// Реконструкция из плоской строки, которую возвращает audio-thread
    /// (там у нас `Result<_, String>` для упрощения channel API).
    pub fn from_engine_string(s: String) -> Self {
        if s == ERR_FEEDBACK_LOOP {
            return Self::feedback_blocked(ERR_FEEDBACK_LOOP);
        }
        if let Some(rest) = s.strip_prefix("device not found: ") {
            return Self::device_not_found(rest);
        }
        if s == "engine not started" {
            return Self::engine_not_started(s);
        }
        if s == "engine thread gone" || s == "engine thread dropped reply" || s == "engine died" {
            return Self::engine_dead(s);
        }
        if s.starts_with("test already playing on ") {
            return Self::test_already_playing(s);
        }
        Self::backend(s)
    }
}

/// Является ли `candidate_name` тем же устройством, с которого идёт
/// loopback-захват. `loopback_source = None` → capture не запущен,
/// блокировки не нужны.
///
/// Сравнение по имени, потому что в MVP DeviceId == имя устройства
/// (см. `zound_platform::open_output_by_name`).
pub fn is_capture_source(loopback_source: Option<&str>, candidate_name: &str) -> bool {
    matches!(loopback_source, Some(src) if src == candidate_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_capture_means_anything_is_allowed() {
        assert!(!is_capture_source(None, "Speakers"));
        assert!(!is_capture_source(None, ""));
    }

    #[test]
    fn matches_only_exact_name() {
        assert!(is_capture_source(Some("Speakers"), "Speakers"));
        assert!(!is_capture_source(Some("Speakers"), "Speaker"));
        assert!(!is_capture_source(Some("Speakers"), "speakers"));
        assert!(!is_capture_source(Some("Speakers"), " Speakers"));
    }

    #[test]
    fn command_error_serializes_with_kind_tag() {
        let e = CommandError::feedback_blocked("x");
        let s = serde_json::to_string(&e).unwrap();
        assert!(s.contains(r#""kind":"feedbackBlocked""#));
        assert!(s.contains(r#""message":"x""#));
    }

    #[test]
    fn from_engine_string_recovers_kinds() {
        assert!(matches!(
            CommandError::from_engine_string(ERR_FEEDBACK_LOOP.into()),
            CommandError::FeedbackBlocked { .. }
        ));
        assert!(matches!(
            CommandError::from_engine_string("device not found: a".into()),
            CommandError::DeviceNotFound { .. }
        ));
        assert!(matches!(
            CommandError::from_engine_string("engine thread gone".into()),
            CommandError::EngineDead { .. }
        ));
        assert!(matches!(
            CommandError::from_engine_string("test already playing on Spkr".into()),
            CommandError::TestAlreadyPlaying { .. }
        ));
        assert!(matches!(
            CommandError::from_engine_string("oops".into()),
            CommandError::Backend { .. }
        ));
    }
}
