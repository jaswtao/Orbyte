//! Recognition of live sudo prompts, without retaining password input.
//!
//! Author: jaswtao
//! Date: 2026-11-01

use serde::Serialize;

#[derive(Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SudoPrompt {
    pub prompt_id: Option<String>,
}

#[derive(Default)]
enum Escape {
    #[default]
    Text,
    Start,
    Csi(Vec<u8>),
    Osc(Vec<u8>, bool),
}

#[derive(Default)]
pub struct SudoDetector {
    line: Vec<u8>,
    escape: Escape,
    prompt: SudoPrompt,
    password_input: bool,
    suppressed: bool,
    attempted: bool,
    shell_prompt: bool,
    alternate_screen: bool,
}

impl SudoDetector {
    pub fn prompt(&self) -> SudoPrompt { self.prompt.clone() }

    pub fn output(&mut self, data: &[u8]) {
        for &byte in data {
            match &mut self.escape {
                Escape::Text => match byte {
                    0x1b => self.escape = Escape::Start,
                    b'\r' | b'\n' => {
                        self.line.clear();
                        self.prompt = SudoPrompt::default();
                        self.suppressed = false;
                    }
                    0x08 => { self.line.pop(); }
                    0x20..=0xff => {
                        if self.line.len() < 4096 { self.line.push(byte); }
                    }
                    _ => {}
                },
                Escape::Start => self.escape = match byte {
                    b'[' => Escape::Csi(Vec::new()),
                    b']' => Escape::Osc(Vec::new(), false),
                    _ => Escape::Text,
                },
                Escape::Csi(parameters) => {
                    if (0x40..=0x7e).contains(&byte) {
                        if matches!(byte, b'h' | b'l') && parameters.first() == Some(&b'?') &&
                            parameters[1..].split(|ch| *ch == b';').any(|mode| matches!(mode, b"47" | b"1047" | b"1049")) {
                            self.alternate_screen = byte == b'h';
                        }
                        // Cursor movement or erasure makes an earlier prompt stale.
                        if byte != b'm' { self.line.clear(); self.prompt = SudoPrompt::default(); }
                        self.escape = Escape::Text;
                    } else if parameters.len() < 64 {
                        parameters.push(byte);
                    }
                }
                Escape::Osc(bytes, escaped) => {
                    if byte == 7 || (*escaped && byte == b'\\') {
                        if bytes.starts_with(b"9;9;") || bytes == b"133;A" {
                            self.command_boundary();
                        }
                        self.escape = Escape::Text;
                    } else {
                        *escaped = byte == 0x1b;
                        if bytes.len() < 4096 { bytes.push(byte); }
                    }
                }
            }
        }
        let text = String::from_utf8_lossy(&self.line);
        self.shell_prompt = !text.starts_with("[sudo]") && (text.ends_with("$ ") || text.ends_with("# ") || text.ends_with("% "));
        // Continued output invalidates a prompt even without a newline.
        if !standard_prompt(&text) { self.prompt = SudoPrompt::default(); }
    }

    /// Called only after all immediately available output has been consumed.
    pub fn settle(&mut self) -> bool {
        if self.alternate_screen || self.suppressed || self.attempted || !standard_prompt(&String::from_utf8_lossy(&self.line)) { return false; }
        self.password_input = true;
        if self.prompt.prompt_id.is_some() { return false; }
        self.prompt.prompt_id = Some(uuid::Uuid::new_v4().to_string());
        true
    }

    pub fn input(&mut self, data: &[u8]) {
        if data.is_empty() { return; }
        if data.contains(&3) {
            self.command_boundary();
            return;
        }
        if self.shell_prompt {
            self.command_boundary();
        }
        if self.password_input || self.prompt.prompt_id.is_some() {
            self.suppressed = true;
            self.prompt = SudoPrompt::default();
            if data.contains(&b'\r') || data.contains(&b'\n') { self.line.clear(); }
        }
    }

    pub fn consume(&mut self, id: &str) -> bool {
        if self.prompt.prompt_id.as_deref() != Some(id) { return false; }
        // One automatic submission per command line; retries and chained sudo calls need manual input.
        self.attempted = true;
        self.suppressed = true;
        self.prompt = SudoPrompt::default();
        self.line.clear();
        true
    }

    pub fn disable(&mut self) {
        self.suppressed = true;
        self.prompt = SudoPrompt::default();
    }

    fn command_boundary(&mut self) {
        self.line.clear();
        self.prompt = SudoPrompt::default();
        self.password_input = false;
        self.suppressed = false;
        self.attempted = false;
        self.shell_prompt = false;
    }
}

fn standard_prompt(line: &str) -> bool {
    let Some(text) = line.trim_end().strip_prefix("[sudo] ") else { return false; };
    if let Some(user) = text.strip_prefix("password for ").and_then(|value| value.strip_suffix(':')) {
        return valid_user(user);
    }
    for suffix in [" 的密码：", " 的密码:", "的密码：", "的密码:"] {
        if let Some(user) = text.strip_suffix(suffix) { return valid_user(user); }
    }
    false
}

fn valid_user(user: &str) -> bool {
    !user.is_empty() && !user.chars().any(|ch| ch.is_whitespace() || ch.is_control() || matches!(ch, ':' | '：'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragmented_standard_prompts_ignore_ansi_and_keep_the_same_id() {
        for text in ["[sudo] password for tao:", "[sudo] tao 的密码：", "[sudo] tao 的密码:"] {
            let mut detector = SudoDetector::default();
            for byte in format!("\x1b[33m{text}\x1b[0m ").bytes() { detector.output(&[byte]); }
            assert!(detector.settle());
            let id = detector.prompt();
            assert!(!detector.settle());
            assert!(detector.prompt() == id);
        }
    }

    #[test]
    fn historical_custom_and_partial_output_are_not_prompts() {
        for text in ["[sudo] password for tao:\r\n", "password:", "[sudo] password for ", "log [sudo] password for tao:"] {
            let mut detector = SudoDetector::default();
            detector.output(text.as_bytes());
            assert!(!detector.settle());
        }
    }

    #[test]
    fn manual_input_never_reenables_after_backspace() {
        let mut detector = SudoDetector::default();
        detector.output(b"[sudo] password for tao: ");
        detector.settle();
        detector.input(b"secret\x7f");
        assert!(detector.prompt().prompt_id.is_none());
        detector.output(b" ");
        assert!(!detector.settle());
    }

    #[test]
    fn saved_password_is_single_use_until_next_command() {
        let mut detector = SudoDetector::default();
        detector.output(b"[sudo] password for tao: ");
        detector.settle();
        let id = detector.prompt().prompt_id.unwrap();
        assert!(!detector.consume("stale"));
        assert!(detector.consume(&id));
        assert!(!detector.consume(&id));
        detector.output(b"\r\nSorry, try again.\r\n[sudo] password for tao: ");
        assert!(!detector.settle());
        detector.output(b"\r\n\x1b]9;9;/home/tao\x07$ ");
        detector.input(b"sudo id\r");
        detector.output(b"sudo id\r\n[sudo] password for tao: ");
        assert!(detector.settle());
    }

    #[test]
    fn cancel_and_continued_output_invalidate_old_ids() {
        let mut detector = SudoDetector::default();
        detector.output(b"[sudo] password for tao: ");
        detector.settle();
        let id = detector.prompt().prompt_id.unwrap();
        detector.output(b"this is a log\r\n");
        assert!(!detector.consume(&id));
        detector.input(&[3]);
        detector.output(b"[sudo] password for tao: ");
        assert!(detector.settle());
    }

    #[test]
    fn pager_output_does_not_offer_credentials_and_plain_shell_prompts_allow_new_commands() {
        let mut detector = SudoDetector::default();
        detector.output(b"\x1b[?1049h[sudo] password for tao: ");
        assert!(!detector.settle());
        detector.output(b"\x1b[?1049l\r\n[sudo] password for tao: ");
        assert!(detector.settle());
        let id = detector.prompt().prompt_id.unwrap();
        assert!(detector.consume(&id));
        detector.output(b"\r\nuser@host:~$ ");
        detector.input(b"s");
        detector.output(b"sudo id\r\n[sudo] password for tao: ");
        assert!(detector.settle());
    }
}
