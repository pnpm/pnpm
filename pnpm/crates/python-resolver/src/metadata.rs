use miette::{Result, bail};
use serde::{Deserialize, Serialize};

/// What resolution reads out of a wheel's `METADATA`: what it requires,
/// which extras it offers, and which interpreters it accepts. A caller
/// that reads more (the dist-info directory name, whether the wheel is
/// pure) keeps that to itself.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct WheelMetadata {
    pub name: String,
    pub version: String,
    pub requires_dist: Vec<String>,
    pub requires_python: Option<String>,
    pub provides_extra: Vec<String>,
}

impl WheelMetadata {
    /// Read a wheel's `METADATA` document, whether it came from the index
    /// (PEP 658) or out of the wheel's own dist-info directory.
    ///
    /// The format is RFC 822 headers: one `Name: value` per line, a value
    /// continued on a line that starts with whitespace, and a blank line
    /// ending the headers (the long description follows, which resolution
    /// does not read). Unknown fields are ignored, as the format intends.
    pub fn parse(document: &str) -> Result<Self> {
        let mut metadata = Self::default();
        let mut field: Option<(String, String)> = None;
        for line in document.lines() {
            if line.is_empty() {
                break;
            }
            if let Some(started) = metadata.read_field_line(line, &mut field)? {
                field = Some(started);
            }
        }
        if let Some((name, value)) = field {
            metadata.take_field(&name, value);
        }
        if metadata.name.is_empty() || metadata.version.is_empty() {
            bail!("Python wheel metadata names no distribution");
        }
        Ok(metadata)
    }

    /// Read one metadata line. A continuation line (RFC 822 folding) extends
    /// the field being read; anything else closes it and starts the next,
    /// which is what the caller is handed back.
    fn read_field_line(
        &mut self,
        line: &str,
        field: &mut Option<(String, String)>,
    ) -> Result<Option<(String, String)>> {
        if line.starts_with([' ', '\t']) {
            if let Some((_, value)) = field.as_mut() {
                value.push(' ');
                value.push_str(line.trim());
            }
            return Ok(None);
        }
        if let Some((name, value)) = field.take() {
            self.take_field(&name, value);
        }
        let Some((name, value)) = line.split_once(':') else {
            bail!("Python wheel metadata has a line that is not a field: {line:?}");
        };
        Ok(Some((name.trim().to_ascii_lowercase(), value.trim().to_string())))
    }

    fn take_field(&mut self, name: &str, value: String) {
        match name {
            "name" => self.name = value,
            "version" => self.version = value,
            "requires-dist" => self.requires_dist.push(value),
            "provides-extra" => self.provides_extra.push(value),
            "requires-python" => self.requires_python = Some(value),
            _ => {}
        }
    }
}
