//! Защита от feedback-loop: устройство, на котором сейчас идёт
//! loopback-захват, нельзя ни добавлять как output, ни запускать на нём
//! тест-сигнал — иначе сигнал отыграет в системе → попадёт в наш
//! capture → отыграет снова → бесконечная петля с возрастающей громкостью.
//!
//! Логика тривиальная (сравнение строк), но вынесена в отдельный модуль
//! из соображений тестируемости: реальный путь идёт через cpal-стримы,
//! ловить feedback вживую дорого. Здесь — pure-функции.

/// Маркер ошибки, который audio-thread возвращает наружу в качестве
/// сообщения. Frontend ловит эту строку и показывает локализованное
/// сообщение `feedback-default-blocked` вместо raw-строки.
pub const ERR_FEEDBACK_LOOP: &str = "feedback-default-blocked";

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
}
