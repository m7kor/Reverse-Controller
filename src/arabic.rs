//! Arabic text reshaping and BiDi reordering for egui.
//!
//! egui does not have a built-in HarfBuzz / BiDi shaping engine, so Arabic text
//! passed as raw Unicode codepoints is rendered disconnected and left-to-right.
//! This module reshapes characters into connected cursive presentation forms
//! and reorders RTL runs so they render correctly in egui.

pub fn reshape_ar(text: &str) -> String {
    if !text.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c) || ('\u{0750}'..='\u{077F}').contains(&c) || ('\u{FB50}'..='\u{FDFF}').contains(&c) || ('\u{FE70}'..='\u{FEFF}').contains(&c)) {
        return text.to_string();
    }

    let reshaper = arabic_reshaper::ArabicReshaper::default();
    let reshaped = reshaper.reshape(text);
    let bidi_info = unicode_bidi::BidiInfo::new(&reshaped, Some(unicode_bidi::Level::ltr()));
    let mut out = String::new();
    for para in &bidi_info.paragraphs {
        let line = para.range.clone();
        out.push_str(&bidi_info.reorder_line(para, line));
    }
    if out.is_empty() {
        reshaped
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arabic_reshaping() {
        let res = reshape_ar("النقر الأيسر");
        assert!(!res.is_empty());
        // Verify that Arabic characters were transformed to connected Presentation Forms
        assert!(res.chars().any(|c| ('\u{FB50}'..='\u{FDFF}').contains(&c) || ('\u{FE70}'..='\u{FEFF}').contains(&c)));
    }
}
