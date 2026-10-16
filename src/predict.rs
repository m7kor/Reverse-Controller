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

/// Find top matching word predictions for a given prefix.
pub fn predict<'a>(prefix: &str, is_arabic: bool, max_results: usize) -> Vec<&'a str> {
    let trimmed = prefix.trim();
    if trimmed.is_empty() || max_results == 0 {
        return Vec::new();
    }

    let mut matches = Vec::new();
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
            if w.starts_with(&lower_prefix) && !w.eq_ignore_ascii_case(trimmed) {
                matches.push(w);
                if matches.len() >= max_results {
                    break;
                }
            }
        }
    }

    matches
}

#[cfg(test)]
mod tests {
    use super::*;

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
