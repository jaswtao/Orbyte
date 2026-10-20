//! Windows ConPTY 输入兼容：避免弯引号被合成为仅在 Alt 松开时携带字符的事件。
//! Author: jaswtao · Date: 2026-10-15

use std::borrow::Cow;
use std::fmt::Write;

pub(crate) fn encode_input(data: &[u8]) -> Cow<'_, [u8]> {
    let Ok(text) = std::str::from_utf8(data) else {
        return Cow::Borrowed(data);
    };
    if !text.contains(['‘', '’', '“', '”']) {
        return Cow::Borrowed(data);
    }

    let mut encoded = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(ch, '‘' | '’' | '“' | '”') {
            // ConPTY 的 win32-input-mode：Vk;Sc;UnicodeChar;KeyDown;ControlState;Repeat。
            // 直接传递 Unicode 按下/松开，绕过旧式 Alt 数字输入合成；不携带修饰键。
            // https://github.com/microsoft/terminal/blob/main/doc/specs/%234999%20-%20Improved%20keyboard%20handling%20in%20Conpty.md
            let code = ch as u32;
            write!(encoded, "\x1b[0;0;{code};1;0;1_\x1b[0;0;{code};0;0;1_").unwrap();
        } else {
            encoded.push(ch);
        }
    }
    Cow::Owned(encoded.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curly_quotes_use_unicode_key_events_in_original_order() {
        let data = encode_input("甲“乙”‘丙’\"丁\"".as_bytes());
        assert_eq!(data.as_ref(), concat!(
            "甲\x1b[0;0;8220;1;0;1_\x1b[0;0;8220;0;0;1_乙",
            "\x1b[0;0;8221;1;0;1_\x1b[0;0;8221;0;0;1_",
            "\x1b[0;0;8216;1;0;1_\x1b[0;0;8216;0;0;1_丙",
            "\x1b[0;0;8217;1;0;1_\x1b[0;0;8217;0;0;1_\"丁\"",
        ).as_bytes());
    }

    #[test]
    fn other_text_controls_and_binary_are_unchanged() {
        for data in ["中文 abc \"'\r\n\x1b\x7f\x03".as_bytes(), &[0xff, 0xfe], b""] {
            assert!(matches!(encode_input(data), Cow::Borrowed(value) if value == data));
        }
    }

    #[test]
    fn paste_delimiters_and_newlines_are_preserved() {
        let encoded = encode_input("\x1b[200~“第一行”\r‘第二行’\x1b[201~".as_bytes());
        assert!(encoded.starts_with(b"\x1b[200~"));
        assert!(encoded.ends_with(b"\x1b[201~"));
        assert_eq!(encoded.iter().filter(|&&b| b == b'\r').count(), 1);
    }
}
