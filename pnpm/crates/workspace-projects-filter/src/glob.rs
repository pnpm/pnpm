//! Path glob matcher for directory selectors, covering the
//! `micromatch.isMatch(dir, pattern, { format })` call upstream uses for
//! `useGlobDirFiltering` selections.
//!
//! Four wildcards are supported: `*` matches any run of characters within
//! a single path segment, `**` matches any number of whole segments
//! (including zero), `?` matches exactly one character, and `[abc]` /
//! `[a-c]` matches one character from a set. A leading `^` negates a set;
//! a leading `!` does not, because micromatch's picomatch backend reads it
//! as an ordinary member. A candidate equal to the pattern always matches,
//! which is how picomatch keeps a directory whose name contains `[`, `{`
//! or another metacharacter selectable by its own path.
//!
//! `{a,b}` selects either alternative. Alternatives nest, may span `/`,
//! and combine with the wildcards above. Braces holding no top-level comma
//! are literal, except `{x..y}`, which picomatch turns into the character
//! class `[x-y]` rather than expanding a range. An alternative opening with
//! `**` is matched as an ordinary globstar segment; picomatch drops its
//! leading-dot guard and its match-nothing case in that one position.
//!
//! A wildcard does not match a segment's leading `.`, matching micromatch's
//! default `dot: false`. A character class is exempt, as it is upstream:
//! `[.]hidden` and `[a-z.]hidden` select `.hidden`, `?hidden` does not.
//! Upstream drops that guard for a wildcard written inside a brace
//! alternative, so `{*,x}` selects `.hidden` there and not here. A project
//! directory whose name begins with a dot is outside what workspace globs
//! pick up anyway.
//!
//! Both the pattern and the candidate are normalized the same way before
//! matching: backslashes become `/` and a trailing `/` is stripped. This
//! mirrors upstream's pattern `replace(/\\/g, '/')` together with
//! micromatch's separator handling, which treats `\` in the candidate as a
//! path separator too — so a Windows `ProjectRootDir` rendered with
//! backslashes by [`PathBuf::to_string_lossy`](std::path::PathBuf) still
//! matches.

/// A directory glob parsed once and matched against many candidate paths.
/// The [module documentation](self) describes the syntax it accepts.
pub struct DirGlob {
    normalized: String,
    /// One segment list per brace alternative, so `{a,b}` is matched as
    /// the two patterns it stands for.
    alternatives: Vec<Vec<Segment>>,
}

impl DirGlob {
    pub fn new(pattern: &str) -> Self {
        let normalized = normalize(pattern);
        // A `{` left open makes picomatch compile a regex that matches
        // nothing, so the exact-text shortcut in `is_match` is all that is
        // left. No alternative says the same thing.
        let alternatives = if brace_spans(&normalized.chars().collect::<Vec<_>>()).unmatched_open {
            Vec::new()
        } else {
            expand_braces(&normalized)
                .iter()
                .map(|alternative| alternative.split('/').map(Segment::parse).collect())
                .collect()
        };
        DirGlob { normalized, alternatives }
    }

    /// Whether `candidate` matches this glob.
    pub fn is_match(&self, candidate: &str) -> bool {
        let candidate = normalize(candidate);
        if candidate == self.normalized {
            return true;
        }
        let candidate_segments: Vec<&str> = candidate.split('/').collect();
        self.alternatives.iter().any(|segments| match_segments(segments, &candidate_segments))
    }
}

/// Normalize a glob pattern or candidate path: backslashes to `/`, then
/// a single trailing `/` stripped.
fn normalize(path: &str) -> String {
    let path = path.replace('\\', "/");
    match path.strip_suffix('/') {
        Some(stripped) => stripped.to_string(),
        None => path,
    }
}

/// The most alternatives a pattern may expand to. Past this cap the braces
/// stay literal, which bounds both the patterns held at once and the work a
/// selector such as `{a,b}{a,b}{a,b}...` can ask for.
const MAX_ALTERNATIVES: usize = 1024;

/// The deepest brace nesting a pattern may use before its braces are taken
/// as literal text. Real selectors nest a level or two; the limit keeps
/// [`expand_braces`] from recursing arbitrarily deep on a malformed one.
const MAX_BRACE_DEPTH: usize = 32;

/// Expand `{a,b}` alternatives into the patterns they stand for. A pattern
/// without an expandable group, or one whose expansion is refused by
/// [`expand_alternatives`], yields itself.
fn expand_braces(pattern: &str) -> Vec<String> {
    expand_alternatives(pattern).unwrap_or_else(|| vec![pattern.to_string()])
}

/// The patterns `pattern` stands for, or `None` when expanding it would
/// pass [`MAX_ALTERNATIVES`] or [`MAX_BRACE_DEPTH`]. Refusal propagates out
/// of a nested group, so an alternative too wide to expand takes the whole
/// pattern with it rather than leaving its siblings selectable.
///
/// One left-to-right pass carries a growing set of prefixes, so the pattern
/// is scanned once however many groups it holds. Rescanning it per group
/// would be quadratic, and a single-branch group such as `{a..c}` would
/// never reach the cap that otherwise bounds the work.
fn expand_alternatives(pattern: &str) -> Option<Vec<String>> {
    if !pattern.contains('{') {
        return Some(vec![pattern.to_string()]);
    }
    let chars: Vec<char> = pattern.chars().collect();
    let spans = brace_spans(&chars);
    if spans.max_depth > MAX_BRACE_DEPTH {
        return None;
    }

    let mut expanded = vec![String::new()];
    let mut literal_start = 0;
    while let Some(group) = next_brace_group(&chars, &spans, literal_start) {
        // An alternative may hold groups of its own. Nesting is capped
        // above, so this recursion is bounded.
        let mut branches = Vec::new();
        for alternative in &group.alternatives {
            branches.extend(expand_alternatives(alternative)?);
        }
        if expanded.len() * branches.len() > MAX_ALTERNATIVES {
            return None;
        }
        let literal: String = chars[literal_start..group.start].iter().collect();
        expanded = join_branches(&expanded, &literal, &branches);
        literal_start = group.close + 1;
    }

    let tail: String = chars[literal_start..].iter().collect();
    for alternative in &mut expanded {
        alternative.push_str(&tail);
    }
    Some(expanded)
}

/// A brace group that stands for something other than its own text.
struct BraceGroup {
    start: usize,
    close: usize,
    alternatives: Vec<String>,
}

/// The first expandable group at or after `from`. Braces inside a bracket
/// expression, braces left open, and braces holding no top-level comma are
/// all ordinary text, so the scan continues past them.
fn next_brace_group(chars: &[char], spans: &BraceSpans, from: usize) -> Option<BraceGroup> {
    let mut index = from;
    while index < chars.len() {
        if chars[index] == '[' {
            index = bracket_end(chars, &spans.next_close_bracket, index + 1).unwrap_or(index + 1);
            continue;
        }
        let Some(close) = (chars[index] == '{').then(|| spans.closes[index]).flatten() else {
            index += 1;
            continue;
        };
        let content: String = chars[index + 1..close].iter().collect();
        if let Some(alternatives) = brace_alternatives(&content) {
            return Some(BraceGroup { start: index, close, alternatives });
        }
        // The braces are literal, but a group nested inside them still
        // expands: picomatch reads `{{a,b}}` as a literal `{`, the group,
        // and a literal `}`.
        index += 1;
    }
    None
}

/// Every prefix, followed by `literal` and one of `branches`.
fn join_branches(prefixes: &[String], literal: &str, branches: &[String]) -> Vec<String> {
    let mut joined = Vec::with_capacity(prefixes.len() * branches.len());
    for prefix in prefixes {
        for branch in branches {
            joined.push(format!("{prefix}{literal}{branch}"));
        }
    }
    joined
}

/// What a brace group's `content` stands for, or `None` when it is
/// ordinary text.
fn brace_alternatives(content: &str) -> Option<Vec<String>> {
    let parts = split_top_level_commas(content);
    if parts.len() > 1 {
        return Some(parts);
    }
    let (start, end) = split_top_level_range(content)?;
    // picomatch's own reading of a second `..` is incoherent, so leave a
    // group holding one as ordinary text.
    if end.contains("..") {
        return None;
    }
    // A one-sided range is the class of the endpoint it has, so `{a..}` is
    // `[a]`. With both, picomatch orders them: `{x..c}` is `[c-x]`.
    let members = match (start.is_empty(), end.is_empty()) {
        (true, true) => return None,
        (true, false) => end,
        (false, true) => start,
        (false, false) if start <= end => format!("{start}-{end}"),
        (false, false) => format!("{end}-{start}"),
    };
    Some(vec![format!("[{members}]")])
}

/// Split `content` at the `..` of a range: the first one outside any nested
/// brace group or bracket expression. A `..` within a nested group belongs
/// to that group, so `{{a..c}}` holds a range but is not one itself.
fn split_top_level_range(content: &str) -> Option<(String, String)> {
    let chars: Vec<char> = content.chars().collect();
    let spans = brace_spans(&chars);
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '[' => {
                index =
                    bracket_end(&chars, &spans.next_close_bracket, index + 1).unwrap_or(index + 1);
            }
            '{' => index = spans.closes[index].map_or(index + 1, |close| close + 1),
            '.' if chars.get(index + 1) == Some(&'.') => {
                return Some((
                    chars[..index].iter().collect(),
                    chars[index + 2..].iter().collect(),
                ));
            }
            _ => index += 1,
        }
    }
    None
}

/// Split `content` on the commas that separate alternatives: those outside
/// any nested brace group or bracket expression.
fn split_top_level_commas(content: &str) -> Vec<String> {
    let chars: Vec<char> = content.chars().collect();
    let spans = brace_spans(&chars);
    let brackets = &spans.next_close_bracket;
    let mut parts = Vec::new();
    let mut part_start = 0;
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '[' => index = bracket_end(&chars, brackets, index + 1).unwrap_or(index + 1),
            '{' => index = spans.closes[index].map_or(index + 1, |close| close + 1),
            ',' => {
                parts.push(chars[part_start..index].iter().collect());
                index += 1;
                part_start = index;
            }
            _ => index += 1,
        }
    }
    parts.push(chars[part_start..].iter().collect());
    parts
}

/// Where a pattern's brace groups begin and end.
struct BraceSpans {
    /// For each `{`, the index of the `}` that closes it.
    closes: Vec<Option<usize>>,
    /// For each index, the next `]` at or after it, so a bracket
    /// expression's end is a lookup rather than a scan.
    next_close_bracket: Vec<Option<usize>>,
    /// Whether any `{` is left open.
    unmatched_open: bool,
    /// The deepest nesting any group reaches.
    max_depth: usize,
}

/// Locate every brace group in one pass. Scanning once keeps a pattern of
/// many unterminated `{` or `[` linear rather than rescanning the tail for
/// each of them, and leaves no recursion for a deeply nested selector to
/// overflow.
fn brace_spans(chars: &[char]) -> BraceSpans {
    let mut next_close_bracket = vec![None; chars.len()];
    let mut next_bracket = None;
    for index in (0..chars.len()).rev() {
        if chars[index] == ']' {
            next_bracket = Some(index);
        }
        next_close_bracket[index] = next_bracket;
    }

    let mut closes = vec![None; chars.len()];
    let mut open = Vec::new();
    let mut max_depth = 0;
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '[' => {
                index = bracket_end(chars, &next_close_bracket, index + 1).unwrap_or(index + 1);
                continue;
            }
            '{' => {
                open.push(index);
                max_depth = max_depth.max(open.len());
            }
            '}' => {
                if let Some(start) = open.pop() {
                    closes[start] = Some(index);
                }
            }
            _ => {}
        }
        index += 1;
    }
    BraceSpans { closes, next_close_bracket, unmatched_open: !open.is_empty(), max_depth }
}

/// One `/`-delimited part of a pattern.
enum Segment {
    /// `**`, which spans whole segments rather than characters.
    Globstar,
    Tokens(Vec<Token>),
}

impl Segment {
    fn parse(segment: &str) -> Self {
        if segment == "**" {
            return Segment::Globstar;
        }
        let chars: Vec<char> = segment.chars().collect();
        let spans = brace_spans(&chars);
        let mut tokens = Vec::with_capacity(chars.len());
        let mut index = 0;
        while let Some(&character) = chars.get(index) {
            index += 1;
            tokens.push(match character {
                '*' => Token::Star,
                '?' => Token::Char(CharPattern::Any),
                '[' => match CharClass::parse(&chars, &spans.next_close_bracket, index) {
                    Some((class, next)) => {
                        index = next;
                        Token::Char(CharPattern::Class(class))
                    }
                    None => Token::Char(CharPattern::Literal('[')),
                },
                literal => Token::Char(CharPattern::Literal(literal)),
            });
        }
        Segment::Tokens(tokens)
    }
}

/// One element of a parsed pattern segment.
enum Token {
    /// `*`: any (possibly empty) run of characters.
    Star,
    Char(CharPattern),
}

/// A pattern element that consumes exactly one character.
enum CharPattern {
    /// `?`
    Any,
    Literal(char),
    Class(CharClass),
}

impl CharPattern {
    fn matches(&self, character: char) -> bool {
        match self {
            CharPattern::Any => true,
            CharPattern::Literal(literal) => *literal == character,
            CharPattern::Class(class) => class.matches(character),
        }
    }
}

/// A `[...]` bracket expression.
struct CharClass {
    negated: bool,
    members: Vec<ClassMember>,
}

enum ClassMember {
    Char(char),
    Range(char, char),
}

impl CharClass {
    /// Parse the bracket expression whose `[` sits just before `start`,
    /// returning it with the index past its `]`. An unterminated `[` has no
    /// class, and picomatch then treats the `[` as a literal character.
    fn parse(
        chars: &[char],
        next_close_bracket: &[Option<usize>],
        start: usize,
    ) -> Option<(Self, usize)> {
        let past_close = bracket_end(chars, next_close_bracket, start)?;
        let close = past_close - 1;
        let mut index = start;
        let negated = chars[index] == '^';
        if negated {
            index += 1;
        }
        let mut members = Vec::new();
        // A `]` in the first position is a member, not the terminator.
        if chars[index] == ']' {
            members.push(ClassMember::Char(']'));
            index += 1;
        }
        while index < close {
            // `a-c` is a range; a `-` that ends the expression is a member.
            match chars.get(index + 1) {
                Some('-') if index + 2 < close => {
                    members.push(ClassMember::Range(chars[index], chars[index + 2]));
                    index += 3;
                }
                _ => {
                    members.push(ClassMember::Char(chars[index]));
                    index += 1;
                }
            }
        }
        Some((CharClass { negated, members }, past_close))
    }

    fn matches(&self, character: char) -> bool {
        let contains = self.members.iter().any(|member| match *member {
            ClassMember::Char(member) => member == character,
            ClassMember::Range(start, end) => (start..=end).contains(&character),
        });
        contains != self.negated
    }
}

/// The index just past the `]` closing the bracket expression opened just
/// before `start`, or `None` when it is unterminated. A `^` and then a `]`
/// right after the `[` are part of the expression rather than its end.
/// `next_close_bracket` is [`BraceSpans::next_close_bracket`], which makes
/// this a lookup rather than a scan.
fn bracket_end(
    chars: &[char],
    next_close_bracket: &[Option<usize>],
    start: usize,
) -> Option<usize> {
    let mut index = start;
    if chars.get(index) == Some(&'^') {
        index += 1;
    }
    if chars.get(index) == Some(&']') {
        index += 1;
    }
    next_close_bracket.get(index).copied().flatten().map(|close| close + 1)
}

fn match_segments(pattern: &[Segment], candidate: &[&str]) -> bool {
    match pattern.split_first() {
        None => candidate.is_empty(),
        Some((Segment::Globstar, rest)) => (0..=candidate.len())
            .take_while(|&skip| skip == 0 || !candidate[skip - 1].starts_with('.'))
            .any(|skip| match_segments(rest, &candidate[skip..])),
        Some((Segment::Tokens(tokens), rest)) => match candidate.split_first() {
            Some((head, tail)) if segment_match(tokens, head) => match_segments(rest, tail),
            _ => false,
        },
    }
}

/// Match a single pattern segment against a single candidate segment. Uses
/// the classic iterative wildcard match with backtracking so multiple `*`
/// in one segment (`a*b*c`) match correctly.
fn segment_match(pattern: &[Token], text: &str) -> bool {
    let leading_wildcard =
        matches!(pattern.first(), Some(Token::Star | Token::Char(CharPattern::Any)));
    if leading_wildcard && text.starts_with('.') {
        return false;
    }
    let text: Vec<char> = text.chars().collect();
    let (mut pat, mut txt) = (0usize, 0usize);
    // The last `*` seen and the text position it was matched against, so
    // a failed match can backtrack and let the `*` consume one more char.
    let mut backtrack: Option<(usize, usize)> = None;

    while txt < text.len() {
        match pattern.get(pat) {
            Some(Token::Star) => {
                backtrack = Some((pat, txt));
                pat += 1;
            }
            Some(Token::Char(char_pattern)) if char_pattern.matches(text[txt]) => {
                pat += 1;
                txt += 1;
            }
            _ => match backtrack {
                Some((star_pat, star_txt)) => {
                    pat = star_pat + 1;
                    txt = star_txt + 1;
                    backtrack = Some((star_pat, txt));
                }
                None => return false,
            },
        }
    }
    while matches!(pattern.get(pat), Some(Token::Star)) {
        pat += 1;
    }
    pat == pattern.len()
}

#[cfg(test)]
mod tests;
