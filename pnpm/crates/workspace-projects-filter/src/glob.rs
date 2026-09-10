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
//! A wildcard does not match a segment's leading `.`, matching micromatch's
//! default `dot: false`. A character class is exempt, as it is upstream:
//! `[.]hidden` and `[a-z.]hidden` select `.hidden`, `?hidden` does not.
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
    segments: Vec<Segment>,
}

impl DirGlob {
    pub fn new(pattern: &str) -> Self {
        let normalized = normalize(pattern);
        let segments = normalized.split('/').map(Segment::parse).collect();
        DirGlob { normalized, segments }
    }

    /// Whether `candidate` matches this glob.
    pub fn is_match(&self, candidate: &str) -> bool {
        let candidate = normalize(candidate);
        if candidate == self.normalized {
            return true;
        }
        let candidate_segments: Vec<&str> = candidate.split('/').collect();
        match_segments(&self.segments, &candidate_segments)
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
        let mut tokens = Vec::with_capacity(chars.len());
        let mut index = 0;
        while let Some(&character) = chars.get(index) {
            index += 1;
            tokens.push(match character {
                '*' => Token::Star,
                '?' => Token::Char(CharPattern::Any),
                '[' => match CharClass::parse(&chars, index) {
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
    fn parse(chars: &[char], start: usize) -> Option<(Self, usize)> {
        let mut index = start;
        let negated = chars.get(index) == Some(&'^');
        if negated {
            index += 1;
        }
        let mut members = Vec::new();
        // A `]` in the first position is a member, not the terminator.
        if chars.get(index) == Some(&']') {
            members.push(ClassMember::Char(']'));
            index += 1;
        }
        while let Some(&character) = chars.get(index) {
            if character == ']' {
                return Some((CharClass { negated, members }, index + 1));
            }
            // `a-c` is a range; a `-` that ends the expression is a member.
            match chars.get(index + 1) {
                Some('-') if chars.get(index + 2).is_some_and(|&end| end != ']') => {
                    members.push(ClassMember::Range(character, chars[index + 2]));
                    index += 3;
                }
                _ => {
                    members.push(ClassMember::Char(character));
                    index += 1;
                }
            }
        }
        None
    }

    fn matches(&self, character: char) -> bool {
        let contains = self.members.iter().any(|member| match *member {
            ClassMember::Char(member) => member == character,
            ClassMember::Range(start, end) => (start..=end).contains(&character),
        });
        contains != self.negated
    }
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
