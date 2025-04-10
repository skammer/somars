use fluent::{FluentBundle, FluentResource};
use fluent_langneg::{negotiate_languages, NegotiationStrategy};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::{Mutex, RwLock};
use unic_langid::{langid, LanguageIdentifier};

// Define supported languages
pub static LOCALES: Lazy<Vec<LanguageIdentifier>> = Lazy::new(|| vec![
    langid!("en"),
    langid!("ru"),
]);

// Default language
pub static DEFAULT_LOCALE: Lazy<LanguageIdentifier> = Lazy::new(|| langid!("en"));

// Store bundles for each locale - using Mutex instead of RwLock for thread safety
static BUNDLES: Lazy<Mutex<HashMap<String, FluentBundle<FluentResource>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

// Current locale
static CURRENT_LOCALE: Lazy<RwLock<String>> = Lazy::new(|| RwLock::new("en".to_string()));

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
    let mut bundles = BUNDLES.lock().unwrap();
    bundles.insert("en".to_string(), en_bundle);
    bundles.insert("ru".to_string(), ru_bundle);
}

// Set current locale based on requested languages
pub fn set_locale(requested: &[&str]) {
    // Simplified approach - just use the first requested locale if supported
    if let Some(locale) = requested.first() {
        let locale_str = locale.to_string();
        if locale_str == "en" || locale_str == "ru" {
            let mut current = CURRENT_LOCALE.write().unwrap();
            *current = locale_str;
        }
    }
}

// Get translation for a key
pub fn get_message(key: &str) -> String {
    let current_locale = CURRENT_LOCALE.read().unwrap().clone();
    let bundles = BUNDLES.lock().unwrap();
    
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
    CURRENT_LOCALE.read().unwrap().clone()
}

// Shorthand function for translation
pub fn t(key: &str) -> String {
    get_message(key)
}
