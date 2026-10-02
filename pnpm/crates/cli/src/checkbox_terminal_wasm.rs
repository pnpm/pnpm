use console::Key;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{fmt::Write as _, io};

pub(super) struct Term {
    handle: u32,
    size: Option<(u16, u16)>,
}

impl Term {
    pub(super) fn open() -> io::Result<Self> {
        #[derive(Deserialize)]
        struct Opened {
            handle: u32,
            rows: Option<u16>,
            columns: Option<u16>,
        }
        let response = request(&json!({"operation": "terminal.open"}))?;
        let opened: Opened = serde_json::from_value(response).map_err(io::Error::other)?;
        Ok(Self { handle: opened.handle, size: opened.rows.zip(opened.columns) })
    }

    pub(super) fn size_checked(&self) -> Option<(u16, u16)> {
        self.size
    }

    pub(super) fn size(&self) -> (u16, u16) {
        self.size.unwrap_or((24, 80))
    }

    pub(super) fn hide_cursor(&self) -> io::Result<()> {
        self.write("\x1b[?25l")
    }

    pub(super) fn show_cursor(&self) -> io::Result<()> {
        self.write("\x1b[?25h")
    }

    pub(super) fn flush(&self) -> io::Result<()> {
        self.write("")
    }

    pub(super) fn clear_last_lines(&self, rows: usize) -> io::Result<()> {
        if rows == 0 {
            return Ok(());
        }
        let mut output = format!("\x1b[{rows}A");
        for _ in 0..rows {
            output.push_str("\r\x1b[2K\x1b[1B");
        }
        write!(output, "\x1b[{rows}A").expect("writing to a String cannot fail");
        self.write(&output)
    }

    pub(super) fn write_line(&self, line: &str) -> io::Result<()> {
        self.write(&format!("{line}\n"))
    }

    fn write(&self, text: &str) -> io::Result<()> {
        request(&json!({"operation": "terminal.write", "handle": self.handle, "text": text}))?;
        Ok(())
    }

    pub(super) fn read_key_raw(&self) -> io::Result<Key> {
        let response = request(&json!({"operation": "terminal.readKey", "handle": self.handle}))?;
        match response.get("name").and_then(Value::as_str) {
            Some("enter") => Ok(Key::Enter),
            Some("up") => Ok(Key::ArrowUp),
            Some("down") => Ok(Key::ArrowDown),
            Some("ctrl-c") => Ok(Key::CtrlC),
            Some("escape") => Ok(Key::Escape),
            Some("tab") => Ok(Key::Tab),
            Some("backtab") => Ok(Key::BackTab),
            Some("left") => Ok(Key::ArrowLeft),
            Some("right") => Ok(Key::ArrowRight),
            Some("home") => Ok(Key::Home),
            Some("end") => Ok(Key::End),
            Some(_) => Ok(response
                .get("character")
                .and_then(Value::as_str)
                .and_then(|value| value.chars().next())
                .map_or(Key::Unknown, Key::Char)),
            None => Err(io::Error::other("Invalid terminal key response")),
        }
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        if let Err(error) = pnpm_wasm_host::close_resource(self.handle) {
            eprintln!("Failed to restore terminal: {error}");
        }
    }
}

pub(crate) fn request(message: &Value) -> io::Result<Value> {
    pnpm_wasm_host::request_blocking(message).map_err(io::Error::other)
}
