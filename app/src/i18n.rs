//! i18n на основе Project Fluent.
//!
//! Шаблоны лежат в `locales/*.ftl` и встраиваются в бинарь через
//! `include_str!`. Для простоты UI, frontend запрашивает весь словарь
//! за раз (`load_dictionary`); строки с переменными отдаются вместе с
//! placeholder-ами (`{ $ms }`), которые frontend подставляет сам.

use std::collections::HashMap;

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use parking_lot::RwLock;
use unic_langid::langid;

/// Ключи, которые Frontend ждёт в словаре. Держим явный список, чтобы
/// перевод гарантированно покрывал весь UI и ломалось на этапе review,
/// а не в рантайме.
const KEYS: &[&str] = &[
    "app-title",
    "app-subtitle",
    "nav-devices",
    "nav-sync",
    "nav-settings",
    "engine-start",
    "engine-stop",
    "device-add",
    "device-remove",
    "device-source-badge",
    "device-source-note",
    "device-input-only-badge",
    "device-input-only-note",
    "show-all-devices",
    "no-active-outputs",
    "doubling-note",
    "sync-hint",
    "sync-target-latency",
    "volume-label",
    "latency-label",
    "language-label",
    "language-ru",
    "language-en",
    "status-ready",
    "status-engine-started",
    "status-engine-stopped",
    "status-output-added",
    "status-output-removed",
    "status-devices-refreshed",
    "feedback-default-blocked",
];

type Bundle = FluentBundle<FluentResource>;

pub struct I18n {
    bundles: HashMap<String, Bundle>,
    /// Текущий язык — запоминается, но frontend всегда передаёт lang явно.
    /// Держим его здесь, чтобы backend мог логировать и, при желании,
    /// в будущем использовать для push-уведомлений.
    current: RwLock<String>,
}

impl I18n {
    pub fn new() -> Result<Self, String> {
        let mut bundles = HashMap::new();

        bundles.insert(
            "ru".to_string(),
            build_bundle("ru", include_str!("../../locales/ru.ftl"))?,
        );
        bundles.insert(
            "en".to_string(),
            build_bundle("en", include_str!("../../locales/en.ftl"))?,
        );

        Ok(Self {
            bundles,
            current: RwLock::new("ru".to_string()),
        })
    }

    pub fn set_language(&self, lang: &str) {
        if self.bundles.contains_key(lang) {
            *self.current.write() = lang.to_string();
        }
    }

    /// Вернуть все переводы для указанного языка. Если язык неизвестен —
    /// используем английский как fallback.
    pub fn dictionary(&self, lang: &str) -> HashMap<String, String> {
        let bundle = self
            .bundles
            .get(lang)
            .or_else(|| self.bundles.get("en"))
            .expect("no fallback bundle");

        let mut dict = HashMap::with_capacity(KEYS.len());
        let args = FluentArgs::new();
        for &key in KEYS {
            if let Some(msg) = bundle.get_message(key) {
                if let Some(pattern) = msg.value() {
                    let mut errors = Vec::new();
                    let s = bundle.format_pattern(pattern, Some(&args), &mut errors);
                    dict.insert(key.to_string(), s.into_owned());
                }
            } else {
                tracing::warn!(key, lang, "missing translation");
            }
        }
        dict
    }

    /// Форматировать одно сообщение с аргументами. Используется, когда
    /// нужно подставить переменную (например, `sync-target-latency` с `$ms`).
    pub fn format(&self, lang: &str, key: &str, args: &HashMap<String, f64>) -> Option<String> {
        let bundle = self.bundles.get(lang).or_else(|| self.bundles.get("en"))?;
        let msg = bundle.get_message(key)?;
        let pattern = msg.value()?;
        let mut fargs = FluentArgs::new();
        for (k, v) in args {
            fargs.set(k.clone(), FluentValue::from(*v));
        }
        let mut errors = Vec::new();
        Some(
            bundle
                .format_pattern(pattern, Some(&fargs), &mut errors)
                .into_owned(),
        )
    }
}

fn build_bundle(lang: &str, src: &str) -> Result<Bundle, String> {
    let resource = FluentResource::try_new(src.to_string())
        .map_err(|(_, errs)| format!("FTL parse ({lang}): {errs:?}"))?;
    let langid = match lang {
        "ru" => langid!("ru"),
        "en" => langid!("en"),
        other => return Err(format!("unsupported language: {other}")),
    };
    let mut bundle = FluentBundle::new_concurrent(vec![langid]);
    bundle
        .add_resource(resource)
        .map_err(|errs| format!("add_resource ({lang}): {errs:?}"))?;
    // Отключаем unicode isolating marks (u2068/u2069) — они ломают HTML.
    bundle.set_use_isolating(false);
    Ok(bundle)
}
