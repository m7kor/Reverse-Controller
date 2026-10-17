//! Lightweight, zero-dependency word prediction and autocomplete for English and Arabic.
//!
//! Provides instant word suggestions as letters are typed on the on-screen keyboard,
//! dramatically accelerating text entry on Windows without a physical keyboard.

/// English high-frequency word frequency list.
static EN_WORDS: &[&str] = &[
    "the", "be", "to", "of", "and", "a", "in", "that", "have", "i", "it", "for", "not", "on", "with",
    "he", "as", "you", "do", "at", "this", "but", "his", "by", "from", "they", "we", "say", "her",
    "she", "or", "an", "will", "my", "one", "all", "would", "there", "their", "what", "so", "up",
    "out", "if", "about", "who", "get", "which", "go", "me", "when", "make", "can", "like", "time",
    "no", "just", "him", "know", "take", "people", "into", "year", "your", "good", "some", "could",
    "them", "see", "other", "than", "then", "now", "look", "only", "come", "its", "over", "think",
    "also", "back", "after", "use", "two", "how", "our", "work", "first", "well", "way", "even",
    "new", "want", "because", "any", "these", "give", "day", "most", "us", "great", "should",
    "need", "system", "program", "computer", "windows", "controller", "mouse", "keyboard",
    "screen", "display", "setting", "open", "close", "start", "stop", "search", "file", "folder",
    "browser", "internet", "video", "music", "game", "play", "pause", "volume", "sound", "download",
    "upload", "message", "email", "password", "login", "username", "account", "profile", "connect",
    "bluetooth", "device", "button", "click", "select", "cancel", "confirm", "delete", "remove",
    "clear", "save", "apply", "refresh", "reload", "update", "version", "install", "desktop",
    "home", "help", "about", "menu", "window", "tab", "copy", "paste", "cut", "undo", "redo",
    "fast", "slow", "speed", "test", "check", "ready", "done", "please", "thank", "thanks", "hello",
    "welcome", "google", "youtube", "discord", "steam", "reddit", "twitter", "github", "microsoft",
];

/// Arabic high-frequency vocabulary list.
static AR_WORDS: &[&str] = &[
    "في", "من", "على", "إلى", "أن", "هذا", "التي", "الذي", "عن", "الله", "ما", "كان", "هو", "مع",
    "لا", "أو", "كل", "هذه", "بعد", "تم", "بين", "ذلك", "قال", "قد", "يكون", "حيث", "أنه", "حتى",
    "غير", "إذا", "قبل", "عند", "أنا", "أنت", "نحن", "كيف", "لماذا", "متى", "أين", "نعم", "شكرا",
    "السلام", "عليكم", "صباح", "مساء", "الخير", "اليوم", "غدا", "الآن", "دائما", "كبير", "صغير",
    "جديد", "جميل", "سريع", "ممتاز", "تمام", "إن", "شاء", "بسم", "الحمد", "لله", "رب", "العالمين",
    "برنامج", "كمبيوتر", "ويندوز", "ملف", "مجلد", "شاشة", "صوت", "فيديو", "موسيقى", "تطبيق",
    "متصفح", "إنترنت", "بحث", "فتح", "إغلاق", "بدء", "إيقاف", "إعدادات", "حساب", "كلمة", "مرور",
    "تسجيل", "دخول", "رسالة", "محادثة", "نسخ", "لصق", "قص", "تراجع", "تحديد", "الكل", "لوحة",
    "مفاتيح", "فأرة", "يد", "تحكم", "زر", "تأكيد", "إلغاء", "حفظ", "تطبيق", "تحديث", "تحميل",
    "تنزيل", "موقع", "رابط", "صفحة", "سطح", "المكتب", "نافذة", "قائمة", "تشغيل", "ألعاب", "لعبة",
    "وضع", "قناص", "دقيق", "سحب", "نقر", "مؤشر", "سرعة", "بطيء", "عالي", "منخفض", "تغيير", "تبديل",
    "مرحبا", "أهلا", "سهلا", "مع", "سلامة", "عفوا", "أرجو", "ممكن", "مهم", "ضروري", "طريقة", "عمل",
];

/// Normalize Arabic letters (e.g. أ/إ/آ -> ا) and strip tashkeel diacritics for robust prefix matching.
fn normalize_ar(c: char) -> Option<char> {
    if ('\u{064B}'..='\u{065F}').contains(&c) || c == '\u{0670}' {
        return None;
    }
    Some(match c {
        'أ' | 'إ' | 'آ' | 'ٱ' => 'ا',
        'ة' => 'ه',
        'ى' => 'ي',
        _ => c,
    })
}

/// Find top matching word predictions for a given prefix: words the user has
/// typed before come first, then the built-in vocabulary.
pub fn predict(prefix: &str, is_arabic: bool, max_results: usize) -> Vec<&'static str> {
    let trimmed = prefix.trim();
    if trimmed.is_empty() || max_results == 0 {
        return Vec::new();
    }

    let mut matches = learned_matches(trimmed, is_arabic, max_results);
    if matches.len() >= max_results {
        return matches;
    }
    if is_arabic {
        let norm_prefix: String = trimmed.chars().filter_map(normalize_ar).collect();
        if norm_prefix.is_empty() { return matches; }
        for &w in AR_WORDS {
            let norm_w: String = w.chars().filter_map(normalize_ar).collect();
            if norm_w.starts_with(&norm_prefix) && w != trimmed && !matches.contains(&w) {
                matches.push(w);
                if matches.len() >= max_results {
                    break;
                }
            }
        }
    } else {
        let lower_prefix = trimmed.to_lowercase();
        for &w in EN_WORDS {
            if w.starts_with(&lower_prefix) && !w.eq_ignore_ascii_case(trimmed) && !matches.contains(&w) {
                matches.push(w);
                if matches.len() >= max_results {
                    break;
                }
            }
        }
    }

    matches
}

// ---------------------------------------------------------------------------
// Personal word memory
// ---------------------------------------------------------------------------

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// A word must be typed this many times before it is suggested, so a single
/// typo never becomes a suggestion.
const MIN_COUNT_TO_SUGGEST: u32 = 2;
/// Most distinct words remembered; the least used are forgotten first.
const MAX_WORDS: usize = 2000;
/// Save after this many learned words (and always on exit).
const SAVE_EVERY: u32 = 12;

fn is_arabic_char(c: char) -> bool {
    ('\u{0600}'..='\u{06FF}').contains(&c)
}

/// Words the user has typed on the on-screen keyboard, with how often.
#[derive(Default, Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WordMemory {
    counts: HashMap<String, u32>,
}

impl WordMemory {
    /// Normalised form to remember, or `None` for things that are not words.
    fn key(word: &str) -> Option<String> {
        let w = word.trim_matches(|c: char| !c.is_alphanumeric());
        let len = w.chars().count();
        let wordlike = w.chars().all(|c| c.is_alphabetic() || ('\u{064B}'..='\u{065F}').contains(&c));
        if !(3..=24).contains(&len) || !wordlike {
            return None;
        }
        Some(w.to_lowercase())
    }

    pub fn learn(&mut self, word: &str) -> bool {
        let Some(key) = Self::key(word) else {
            return false;
        };
        *self.counts.entry(key).or_insert(0) += 1;
        if self.counts.len() > MAX_WORDS {
            // Forget the least-used word (ties: the longest, rarely retyped).
            if let Some(victim) = self
                .counts
                .iter()
                .min_by_key(|(w, &c)| (c, std::cmp::Reverse(w.len())))
                .map(|(w, _)| w.clone())
            {
                self.counts.remove(&victim);
            }
        }
        true
    }

    /// Remembered words starting with `prefix`, most used first.
    pub fn matches(&self, prefix: &str, is_arabic: bool, max: usize) -> Vec<String> {
        let norm = |s: &str| -> String {
            if is_arabic {
                s.chars().filter_map(normalize_ar).collect()
            } else {
                s.to_lowercase()
            }
        };
        let want = norm(prefix.trim());
        if want.is_empty() {
            return Vec::new();
        }
        let mut hits: Vec<(&String, u32)> = self
            .counts
            .iter()
            .filter(|(w, &c)| c >= MIN_COUNT_TO_SUGGEST && w.chars().any(is_arabic_char) == is_arabic)
            .filter(|(w, _)| {
                let n = norm(w);
                n.starts_with(&want) && n != want
            })
            .map(|(w, &c)| (w, c))
            .collect();
        hits.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        hits.into_iter().take(max).map(|(w, _)| w.clone()).collect()
    }

    pub fn len(&self) -> usize {
        self.counts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }
}

struct MemoryState {
    memory: WordMemory,
    path: PathBuf,
    /// The file existed when loaded or has been written since; if it then
    /// disappears, the user cleared their words from Settings.
    file_known: bool,
    unsaved: u32,
}

static MEMORY: Mutex<Option<MemoryState>> = Mutex::new(None);

/// Load remembered words from `path` and start learning.
pub fn init_learning(path: PathBuf) {
    let loaded = std::fs::read(&path)
        .ok()
        .and_then(|data| serde_json::from_slice::<WordMemory>(&data).ok());
    let file_known = loaded.is_some();
    if let Ok(mut g) = MEMORY.lock() {
        *g = Some(MemoryState { memory: loaded.unwrap_or_default(), path, file_known, unsaved: 0 });
    }
}

/// Remember a finished word. Does nothing until [`init_learning`] ran.
pub fn learn(word: &str) {
    let due = MEMORY
        .lock()
        .ok()
        .and_then(|mut g| {
            let st = g.as_mut()?;
            st.memory.learn(word).then(|| {
                st.unsaved += 1;
                st.unsaved >= SAVE_EVERY
            })
        })
        .unwrap_or(false);
    if due {
        flush();
    }
}

/// Write remembered words to disk if anything changed.
pub fn flush() {
    let Ok(mut g) = MEMORY.lock() else { return };
    let Some(st) = g.as_mut() else { return };
    if st.unsaved == 0 {
        return;
    }
    if st.file_known && !st.path.exists() {
        // Cleared from Settings while running: honour it, don't resurrect.
        st.memory = WordMemory::default();
        st.unsaved = 0;
        st.file_known = false;
        return;
    }
    if let Ok(data) = serde_json::to_vec(&st.memory) {
        if crate::storage::write_atomic(&st.path, &data).is_ok() {
            st.unsaved = 0;
            st.file_known = true;
        }
    }
}

/// Forget every remembered word and delete the file at `path`.
pub fn forget_all(path: &std::path::Path) -> std::io::Result<()> {
    if let Ok(mut g) = MEMORY.lock() {
        if let Some(st) = g.as_mut() {
            st.memory = WordMemory::default();
            st.unsaved = 0;
        }
    }
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Number of words in the file at `path` (for the Settings page).
pub fn learned_count(path: &std::path::Path) -> usize {
    std::fs::read(path)
        .ok()
        .and_then(|data| serde_json::from_slice::<WordMemory>(&data).ok())
        .map(|m| m.len())
        .unwrap_or(0)
}

/// Where the word memory lives: next to config.json.
pub fn memory_path_for(config_path: &std::path::Path) -> PathBuf {
    config_path.with_file_name("learned_words.json")
}

/// Learned words as `'static` strings so they can sit beside the built-in
/// lists. Each distinct word is allocated once; the pool is bounded because
/// the memory itself is.
fn learned_matches(prefix: &str, is_arabic: bool, max: usize) -> Vec<&'static str> {
    type Pool = std::collections::HashSet<&'static str>;
    static POOL: Mutex<Option<Pool>> = Mutex::new(None);
    let words = MEMORY
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|st| st.memory.matches(prefix, is_arabic, max)))
        .unwrap_or_default();
    let Ok(mut pool) = POOL.lock() else { return Vec::new() };
    let pool = pool.get_or_insert_with(Default::default);
    words
        .into_iter()
        .map(|w| match pool.get(w.as_str()) {
            Some(&hit) => hit,
            None => {
                let leaked: &'static str = Box::leak(w.into_boxed_str());
                pool.insert(leaked);
                leaked
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_memory_suggests_repeated_words_most_used_first() {
        let mut m = WordMemory::default();
        for _ in 0..3 {
            m.learn("Discord");
        }
        m.learn("dishwasher");
        m.learn("dishwasher");
        m.learn("distro"); // only once: not suggested yet
        assert_eq!(m.matches("dis", false, 5), vec!["discord", "dishwasher"]);
        assert!(m.matches("discord", false, 5).is_empty(), "the full word is not re-suggested");
    }

    #[test]
    fn word_memory_ignores_non_words_and_keeps_scripts_apart() {
        let mut m = WordMemory::default();
        assert!(!m.learn("ab"));
        assert!(!m.learn("a1b2c3"));
        assert!(!m.learn(&"x".repeat(30)));
        assert!(m.learn("hello,"), "trailing punctuation is trimmed");
        for _ in 0..2 {
            m.learn("مرحبتين");
            m.learn("marhaba");
        }
        assert_eq!(m.matches("مرح", true, 5), vec!["مرحبتين"]);
        assert_eq!(m.matches("mar", false, 5), vec!["marhaba"]);
        assert!(m.matches("mar", true, 5).is_empty());
    }

    #[test]
    fn word_memory_matches_arabic_with_normalised_hamza() {
        let mut m = WordMemory::default();
        m.learn("أستاذية");
        m.learn("أستاذية");
        assert_eq!(m.matches("است", true, 3), vec!["أستاذية"]);
    }

    #[test]
    fn word_memory_is_bounded() {
        let mut m = WordMemory::default();
        for i in 0..(MAX_WORDS + 50) {
            let word: String = format!("{i:05}")
                .chars()
                .map(|c| (b'a' + c.to_digit(10).unwrap() as u8) as char)
                .collect();
            m.learn(&word);
        }
        assert_eq!(m.len(), MAX_WORDS);
    }

    #[test]
    fn word_memory_round_trips_as_json() {
        let mut m = WordMemory::default();
        m.learn("keyboard");
        let back: WordMemory = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn predictions_respect_zero_limit_and_ignore_bare_diacritics() {
        assert!(predict("th", false, 0).is_empty());
        assert!(predict("َ", true, 3).is_empty());
        let results = predict("تط", true, 20);
        assert_eq!(results.iter().filter(|&&word| word == "تطبيق").count(), 1);
    }

    #[test]
    fn predicts_english_words() {
        let res = predict("th", false, 4);
        assert!(!res.is_empty());
        assert!(res.contains(&"the") || res.contains(&"that"));
    }

    #[test]
    fn predicts_arabic_words() {
        let res = predict("الس", true, 4);
        assert!(!res.is_empty());
        assert!(res.contains(&"السلام"));
    }

    #[test]
    fn predicts_arabic_words_with_tashkeel() {
        // "الْسَّ" with sukun and shadda should still predict "السلام"
        let res = predict("الْسَّ", true, 4);
        assert!(!res.is_empty());
        assert!(res.contains(&"السلام"));
    }

    #[test]
    fn empty_prefix_yields_no_results() {
        let res = predict("", false, 4);
        assert!(res.is_empty());
    }
}
