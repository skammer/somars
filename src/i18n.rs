use fluent::{FluentBundle, FluentResource};
use fluent_langneg::{negotiate_languages, NegotiationStrategy};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::RwLock;
use unic_langid::{langid, LanguageIdentifier};

// Define supported languages
pub static LOCALES: Lazy<Vec<LanguageIdentifier>> = Lazy::new(|| vec![
    langid!("en"),
    langid!("ru"),
]);

// Default language
pub static DEFAULT_LOCALE: Lazy<LanguageIdentifier> = Lazy::new(|| langid!("en"));

// Store bundles for each locale
static BUNDLES: Lazy<RwLock<HashMap<LanguageIdentifier, FluentBundle<FluentResource>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

// Current locale
static CURRENT_LOCALE: Lazy<RwLock<LanguageIdentifier>> = Lazy::new(|| RwLock::new(DEFAULT_LOCALE.clone()));

// Initialize i18n system
pub fn init() {
    // English (default)
    let en_ftl = include_str!("../locales/en/main.ftl");
    let en_resource = FluentResource::try_new(en_ftl.to_string())
        .expect("Failed to parse English FTL resource");
    
    let mut en_bundle = FluentBundle::new(vec![langid!("en")]);
    en_bundle.add_resource(en_resource)
        .expect("Failed to add English resource to bundle");
    
    // Russian
    let ru_ftl = include_str!("../locales/ru/main.ftl");
    let ru_resource = FluentResource::try_new(ru_ftl.to_string())
        .expect("Failed to parse Russian FTL resource");
    
    let mut ru_bundle = FluentBundle::new(vec![langid!("ru")]);
    ru_bundle.add_resource(ru_resource)
        .expect("Failed to add Russian resource to bundle");
    
    // Store bundles
    let mut bundles = BUNDLES.write().unwrap();
    bundles.insert(langid!("en"), en_bundle);
    bundles.insert(langid!("ru"), ru_bundle);
}

// Set current locale based on requested languages
pub fn set_locale(requested: &[&str]) {
    let requested_locales: Vec<LanguageIdentifier> = requested
        .iter()
        .filter_map(|locale| locale.parse().ok())
        .collect();
    
    let default = vec![DEFAULT_LOCALE.clone()];
    let available = LOCALES.iter().cloned().collect::<Vec<_>>();
    
    let negotiated = negotiate_languages(
        &requested_locales,
        &available,
        Some(&default),
        NegotiationStrategy::Filtering,
    );
    
    if let Some(locale) = negotiated.first() {
        let mut current = CURRENT_LOCALE.write().unwrap();
        *current = locale.clone();
    }
}

// Get translation for a key
pub fn get_message(key: &str) -> String {
    let current_locale = CURRENT_LOCALE.read().unwrap().clone();
    let bundles = BUNDLES.read().unwrap();
    
    if let Some(bundle) = bundles.get(&current_locale) {
        if let Some(message) = bundle.get_message(key) {
            if let Some(pattern) = message.value() {
                let mut errors = vec![];
                let value = bundle.format_pattern(pattern, None, &mut errors);
                if errors.is_empty() {
                    return value.to_string();
                }
            }
        }
    }
    
    // Fallback to key if translation not found
    key.to_string()
}

// Get current locale code (e.g., "en", "ru")
pub fn get_current_locale_code() -> String {
    let current = CURRENT_LOCALE.read().unwrap();
    current.language.to_string()
}

// Shorthand function for translation
pub fn t(key: &str) -> String {
    get_message(key)
}
