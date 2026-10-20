/// A brace sequence such as `{1..3}`, `{01..10..2}`, or `{a..e}`.
pub(super) struct Sequence {
    start: i128,
    end: i128,
    increment: i128,
    width: usize,
    letters: bool,
}

impl Sequence {
    pub(super) fn parse(content: &str) -> Option<Self> {
        // Reject nesting before splitting to avoid rescanning inner literal groups.
        if content.contains(['{', '}']) {
            return None;
        }
        let mut parts = content.split("..");
        let first = parts.next()?;
        let last = parts.next()?;
        let step_text = parts.next().unwrap_or("1");
        let step = parse_integer(step_text)?.abs().max(1);
        if parts.next().is_some() {
            return None;
        }
        let (start, end, letters) = match (parse_letter(first), parse_letter(last)) {
            (Some(start), Some(end)) => (start, end, true),
            _ => (parse_integer(first)?, parse_integer(last)?, false),
        };
        let width = if !letters && [first, last, step_text].into_iter().any(is_padded) {
            first.len().max(last.len())
        } else {
            0
        };
        Some(Self {
            start,
            end,
            increment: if start <= end { step } else { -step },
            width,
            letters,
        })
    }

    pub(super) fn expand(
        &self,
        prefix: &str,
        suffix: &str,
        max_arms: usize,
        byte_limit: usize,
    ) -> Option<(Vec<String>, usize)> {
        let affixes = prefix.len() + suffix.len() + 1;
        let mut bytes: usize = 0;
        let mut patterns = Vec::new();
        for value in self.values(max_arms) {
            let (sign, digits) = self.unpadded(value);
            // Padded by hand: `{:0width$}` panics on widths above `u16::MAX`.
            let zeros = self.width.saturating_sub(sign.len() + digits.len());
            bytes = bytes.checked_add(affixes + sign.len() + zeros + digits.len())?;
            if bytes > byte_limit {
                return None;
            }
            patterns.push(format!("{prefix}{sign}{}{digits}{suffix}", "0".repeat(zeros)));
        }
        Some((patterns, bytes))
    }

    /// The sign and digits of a number, or the character of a letter sequence.
    /// Letter sequences cross the ASCII punctuation between `Z` and `a`; the
    /// backslash among it expands to nothing, as in `brace-expansion`.
    fn unpadded(&self, value: i128) -> (&'static str, String) {
        if !self.letters {
            let sign = if value < 0 { "-" } else { "" };
            return (sign, value.unsigned_abs().to_string());
        }
        match u8::try_from(value) {
            Ok(b'\\') | Err(_) => ("", String::new()),
            Ok(byte) => ("", char::from(byte).to_string()),
        }
    }

    fn values(&self, limit: usize) -> impl Iterator<Item = i128> + '_ {
        std::iter::successors(Some(self.start), |value| value.checked_add(self.increment))
            .take(limit)
            .take_while(
                |&value| {
                    if self.increment > 0 { value <= self.end } else { value >= self.end }
                },
            )
    }
}

fn parse_integer(text: &str) -> Option<i128> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse::<i64>().ok().map(i128::from)
}

fn parse_letter(text: &str) -> Option<i128> {
    match text.as_bytes() {
        [byte] if byte.is_ascii_alphabetic() => Some(i128::from(*byte)),
        _ => None,
    }
}

fn is_padded(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    digits.len() > 1 && digits.starts_with('0')
}
