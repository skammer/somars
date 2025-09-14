use fluent::{FluentBundle, FluentResource};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::RwLock;
use unic_langid::{langid, LanguageIdentifier};
use std::cell::RefCell;
use std::thread_local;

// Define supported languages
pub static LOCALES: Lazy<Vec<LanguageIdentifier>> = Lazy::new(|| vec![
    langid!("en"),
    langid!("ru"),
]);

// Default language
pub static DEFAULT_LOCALE: Lazy<LanguageIdentifier> = Lazy::new(|| langid!("en"));

// Thread-local storage for bundles
thread_local! {
    static BUNDLES: RefCell<HashMap<String, FluentBundle<FluentResource>>> = RefCell::new(HashMap::new());
}

// Current locale - initialized from environment or defaults to "en"
static CURRENT_LOCALE: Lazy<RwLock<String>> = Lazy::new(|| {
    // Default to English - will be updated during init
    RwLock::new("en".to_string())
});

// Initialize i18n system
pub fn init(preferred_locale: Option<String>) {
    // Set the current locale based on preference or environment
    let locale_to_use = if let Some(locale) = preferred_locale {
        // Use the preferred locale if provided and supported
        if locale == "ru" || locale == "en" {
            locale
        } else {
            // Fallback to environment detection
            detect_locale_from_environment()
        }
    } else {
        // Detect from environment
        detect_locale_from_environment()
    };
    
    // Update the current locale
    {
        let mut current_locale = CURRENT_LOCALE.write().unwrap();
        *current_locale = locale_to_use;
    }
    
    BUNDLES.with(|bundles| {
        let mut bundles = bundles.borrow_mut();
        
        // Only initialize if not already done
        if bundles.is_empty() {
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
            bundles.insert("en".to_string(), en_bundle);
            bundles.insert("ru".to_string(), ru_bundle);
        }
    });
}

// Helper function to detect locale from environment variables
fn detect_locale_from_environment() -> String {
    let env_vars = ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"];
    
    for var in env_vars {
        if let Ok(lang) = std::env::var(var) {
            // Parse the locale string (e.g., "en_US.UTF-8" -> "en")
            let lang_code = lang.split(['_', '.', '@']).next().unwrap_or("").to_lowercase();
            
            // Check if it's a supported locale
            if lang_code == "ru" || lang_code == "en" {
                return lang_code;
            }
        }
    }
    
    // Default to English if not found or not supported
    "en".to_string()
}

// Get translation for a key
pub fn get_message(key: &str) -> String {
    let current_locale = CURRENT_LOCALE.read().unwrap().clone();
    
    let result = BUNDLES.with(|bundles| {
        let bundles = bundles.borrow();
        
        if let Some(bundle) = bundles.get(&current_locale) {
            if let Some(message) = bundle.get_message(key) {
                if let Some(pattern) = message.value() {
                    let mut errors = vec![];
                    let value = bundle.format_pattern(pattern, None, &mut errors);
                    if errors.is_empty() {
                        return Some(value.to_string());
                    }
                }
            }
        }
        None
    });
    
    // Return the result or fallback to key if translation not found
    result.unwrap_or_else(|| key.to_string())
}

// Shorthand function for translation
pub fn t(key: &str) -> String {
    get_message(key)
}
