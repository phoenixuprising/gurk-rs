//! Normalization of displayed text to forms which terminals render with the expected width

use std::borrow::Cow;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const SOFT_HYPHEN: char = '\u{ad}';
const ZWJ: char = '\u{200d}';
const KEYCAP: char = '\u{20e3}';
/// Text presentation selector
const VS15: char = '\u{fe0e}';
/// Emoji presentation selector
const VS16: char = '\u{fe0f}';

/// Emoji modifier bases with text presentation by default
///
/// Terminals like tmux render them 2 columns wide even without [`VS16`], unlike `unicode-width`.
const TEXT_DEFAULT_MODIFIER_BASES: &[char] = &[
    '\u{261d}',
    '\u{26f9}',
    '\u{270c}',
    '\u{270d}',
    '\u{1f3cb}',
    '\u{1f3cc}',
    '\u{1f574}',
    '\u{1f575}',
    '\u{1f590}',
];

/// Rewrites text for display such that terminals agree with `unicode-width` on its width
///
/// Only changes presentation, never the visible content:
///
/// * removes soft hyphens and text presentation selectors (VS15),
/// * removes emoji presentation selectors (VS16) which don't select an emoji,
/// * adds VS16 to text-default emoji in ZWJ sequences and to bare text-default modifier bases
///   (e.g. `✌`), which terminals show as emoji anyway.
///
/// The result is stable, i.e. sanitizing it again does not change it.
pub(crate) fn sanitize(text: &str) -> Cow<'_, str> {
    if text.is_ascii() {
        return Cow::Borrowed(text);
    }
    let mut res = String::with_capacity(text.len());
    for grapheme in text.graphemes(true) {
        sanitize_grapheme(grapheme, &mut res);
    }
    if res == text {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(res)
    }
}

/// Same as [`sanitize`] but for owned strings, avoids allocation if nothing changes
pub(crate) fn sanitize_owned(text: String) -> String {
    let sanitized = match sanitize(&text) {
        Cow::Owned(sanitized) => Some(sanitized),
        Cow::Borrowed(_) => None,
    };
    sanitized.unwrap_or(text)
}

fn sanitize_grapheme(grapheme: &str, res: &mut String) {
    let mut chars = grapheme.chars().peekable();
    let is_zwj_sequence = grapheme.contains(ZWJ);
    let is_single = grapheme.chars().nth(1).is_none();
    let mut prev = None;
    while let Some(c) = chars.next() {
        match c {
            SOFT_HYPHEN | VS15 => continue,
            VS16 => {
                if prev.is_some_and(is_text_default_emoji) || chars.peek() == Some(&KEYCAP) {
                    res.push(c);
                }
                continue;
            }
            _ => {}
        }
        res.push(c);
        let needs_vs16 = (is_zwj_sequence && is_text_default_emoji(c))
            || (is_single && TEXT_DEFAULT_MODIFIER_BASES.contains(&c));
        if needs_vs16 && chars.peek() != Some(&VS16) {
            res.push(VS16);
        }
        prev = Some(c);
    }
}

/// Whether the character is rendered as narrow text, unless followed by VS16 which makes it a wide
/// emoji
fn is_text_default_emoji(c: char) -> bool {
    let mut buf = [0; 8];
    let len = c.encode_utf8(&mut buf).len();
    VS16.encode_utf8(&mut buf[len..]);
    let with_vs16 = std::str::from_utf8(&buf[..len + VS16.len_utf8()]).expect("valid utf8");
    c.width() == Some(1) && with_vs16.width() == 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[track_caller]
    fn check(input: &str, expected: &str) {
        let sanitized = sanitize(input);
        assert_eq!(sanitized, expected, "input: {:?}", input.escape_unicode());
        assert_eq!(sanitize(&sanitized), sanitized, "not idempotent");
    }

    #[test]
    fn test_unchanged() {
        for text in [
            "",
            "hello world",
            "äöü ß é\u{301} 中文 한국어",
            "😀 ❤\u{fe0f} 👍🏽 🇩🇪 1\u{fe0f}\u{20e3}",
            "\u{2764}\u{fe0f}\u{200d}\u{1f525}",
            "\u{1f3f3}\u{fe0f}\u{200d}\u{1f308}",
            "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}",
            "❤ ™ © ↔ ▶",
            "✌\u{fe0f} 🖐🏽",
        ] {
            check(text, text);
            assert!(matches!(sanitize(text), Cow::Borrowed(_)));
        }
    }

    #[test]
    fn test_soft_hyphen() {
        check("hy\u{ad}phen", "hyphen");
    }

    #[test]
    fn test_text_presentation_selector() {
        check("\u{231a}\u{fe0e}", "\u{231a}");
        check("\u{2764}\u{fe0e}", "\u{2764}");
    }

    #[test]
    fn test_emoji_presentation_selector_without_emoji() {
        check("a\u{fe0f}b", "ab");
        check("\u{fe0f}", "");
        check("\u{231a}\u{fe0f}", "\u{231a}");
    }

    #[test]
    fn test_zwj_sequence_without_emoji_presentation() {
        // eye in speech bubble
        check(
            "\u{1f441}\u{200d}\u{1f5e8}",
            "\u{1f441}\u{fe0f}\u{200d}\u{1f5e8}\u{fe0f}",
        );
        check(
            "\u{1f441}\u{fe0f}\u{200d}\u{1f5e8}",
            "\u{1f441}\u{fe0f}\u{200d}\u{1f5e8}\u{fe0f}",
        );
        // heart on fire
        check(
            "\u{2764}\u{200d}\u{1f525}",
            "\u{2764}\u{fe0f}\u{200d}\u{1f525}",
        );
        // rainbow flag
        check(
            "\u{1f3f3}\u{200d}\u{1f308}",
            "\u{1f3f3}\u{fe0f}\u{200d}\u{1f308}",
        );
        // man running
        check(
            "\u{1f3c3}\u{200d}\u{2642}",
            "\u{1f3c3}\u{200d}\u{2642}\u{fe0f}",
        );
    }

    #[test]
    fn test_bare_modifier_base() {
        check("✌", "✌\u{fe0f}");
        check("peace ✌!", "peace ✌\u{fe0f}!");
        check("\u{1f590}", "\u{1f590}\u{fe0f}");
        // with skin tone: left as is
        check("✌🏽", "✌🏽");
    }

    #[test]
    fn test_widths_agree_with_unicode_width() {
        // widths of the sanitized text as measured in tmux 3.7c
        for (text, tmux_width) in [
            ("hy\u{ad}phen", 6),
            ("\u{231a}\u{fe0e}", 2),
            ("a\u{fe0f}", 1),
            ("\u{1f441}\u{200d}\u{1f5e8}", 2),
            ("\u{2764}\u{200d}\u{1f525}", 2),
            ("\u{1f3f3}\u{200d}\u{1f308}", 2),
            ("\u{1f636}\u{200d}\u{1f32b}", 2),
            ("✌", 2),
            ("\u{1f575}", 2),
        ] {
            assert_eq!(
                sanitize(text).width(),
                tmux_width,
                "{:?}",
                text.escape_unicode()
            );
        }
    }
}
