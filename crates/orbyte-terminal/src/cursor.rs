//! 保存光标显隐状态，页面重新订阅时恢复，避免显示 TUI 的绘制光标。
//! Author: jaswtao · Date: 2026-10-14

#[derive(Default)]
enum ParserState {
    #[default]
    Ground,
    Escape,
    Csi(Vec<u8>),
    String {
        osc: bool,
        escape: bool,
    },
}

#[derive(Default)]
pub(crate) struct CursorVisibility {
    hidden: bool,
    parser: ParserState,
}

impl CursorVisibility {
    pub fn restore_sequence(&self) -> &'static [u8] {
        if self.hidden {
            b"\x1b[?25l"
        } else {
            b"\x1b[?25h"
        }
    }

    /// 解析状态跨读取块保留；字符串控制序列中的文本不作为 CSI 指令处理。
    pub fn observe(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.parser = match std::mem::take(&mut self.parser) {
                ParserState::String { osc, escape } => {
                    if (osc && byte == 7)
                        || (escape && byte == b'\\')
                        || matches!(byte, 0x18 | 0x1a)
                    {
                        ParserState::Ground
                    } else {
                        ParserState::String {
                            osc,
                            escape: byte == 0x1b,
                        }
                    }
                }
                _ if byte == 0x1b => ParserState::Escape,
                ParserState::Escape => match byte {
                    b'[' => ParserState::Csi(Vec::new()),
                    b']' | b'P' | b'X' | b'^' | b'_' => ParserState::String {
                        osc: byte == b']',
                        escape: false,
                    },
                    b'c' => {
                        self.hidden = false;
                        ParserState::Ground
                    }
                    _ => ParserState::Ground,
                },
                ParserState::Csi(mut params) => {
                    if (0x40..=0x7e).contains(&byte) {
                        if matches!(byte, b'h' | b'l') && params.first() == Some(&b'?') {
                            if params[1..].split(|&b| b == b';').any(|p| {
                                std::str::from_utf8(p)
                                    .ok()
                                    .and_then(|s| s.parse::<u16>().ok())
                                    == Some(25)
                            }) {
                                self.hidden = byte == b'l';
                            }
                        } else if byte == b'p' && params == b"!" {
                            self.hidden = false;
                        }
                        ParserState::Ground
                    } else if (0x20..=0x3f).contains(&byte) && params.len() < 64 {
                        params.push(byte);
                        ParserState::Csi(params)
                    } else if byte < 0x20 && !matches!(byte, 0x18 | 0x1a) {
                        ParserState::Csi(params)
                    } else {
                        ParserState::Ground
                    }
                }
                ParserState::Ground => ParserState::Ground,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visibility_survives_chunk_boundaries_and_combined_modes() {
        let output = b"\x1b[?25l\x1b[4;1Htest input\x1b[120C";
        for split in 0..=output.len() {
            let mut cursor = CursorVisibility::default();
            cursor.observe(&output[..split]);
            cursor.observe(&output[split..]);
            assert_eq!(cursor.restore_sequence(), b"\x1b[?25l");
            cursor.observe(b"\x1b[?2004;25h");
            assert_eq!(cursor.restore_sequence(), b"\x1b[?25h");
        }
    }

    #[test]
    fn strings_do_not_change_visibility_and_resets_show_cursor() {
        let mut cursor = CursorVisibility::default();
        for string in [
            b"\x1b]title\x1b[?25h\x07".as_slice(),
            b"\x1bP\x1b[?25h\x1b\\".as_slice(),
        ] {
            cursor.observe(b"\x1b[?25l");
            for byte in string {
                cursor.observe(&[*byte]);
            }
            assert_eq!(cursor.restore_sequence(), b"\x1b[?25l");
        }
        for reset in [b"\x1bc".as_slice(), b"\x1b[!p".as_slice()] {
            cursor.observe(b"\x1b[?25l");
            cursor.observe(reset);
            assert_eq!(cursor.restore_sequence(), b"\x1b[?25h");
        }
    }
}
