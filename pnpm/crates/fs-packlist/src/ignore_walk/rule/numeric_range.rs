pub(super) struct NumericRange {
    start: i128,
    end: i128,
    increment: i128,
    width: usize,
}

impl NumericRange {
    pub(super) fn parse(content: &str) -> Option<Self> {
        let mut parts = content.split("..");
        let first = parts.next()?;
        let last = parts.next()?;
        let start = parse_integer(first)?;
        let end = parse_integer(last)?;
        let step = parse_integer(parts.next().unwrap_or("1"))?.abs();
        if parts.next().is_some() || step == 0 {
            return None;
        }
        let width =
            if is_padded(first) || is_padded(last) { first.len().max(last.len()) } else { 0 };
        Some(Self { start, end, increment: if start <= end { step } else { -step }, width })
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
        for value in self.values(max_arms) {
            bytes = bytes.checked_add(affixes + self.width.max(value.to_string().len()))?;
            if bytes > byte_limit {
                return None;
            }
        }
        let patterns = self
            .values(max_arms)
            .map(|value| format!("{prefix}{value:0width$}{suffix}", width = self.width))
            .collect();
        Some((patterns, bytes))
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

fn is_padded(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    digits.len() > 1 && digits.starts_with('0')
}
