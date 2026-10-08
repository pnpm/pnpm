//! One `/`-free segment of an ignore rule: minimatch's wildcards, character
//! classes, and extglobs, matched case-insensitively.

/// A parsed segment.
#[derive(Debug)]
pub(super) struct SegmentPattern {
    nodes: Vec<Node>,
    has_extglob: bool,
}

#[derive(Debug)]
enum Node {
    Literal(char),
    AnyChar,
    AnyRun,
    Class(CharClass),
    Extglob(ExtglobKind, Vec<Vec<Node>>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExtglobKind {
    /// `@(…)`
    One,
    /// `?(…)`
    ZeroOrOne,
    /// `+(…)`
    OneOrMore,
    /// `*(…)`
    ZeroOrMore,
    /// `!(…)`
    Not,
}

#[derive(Debug)]
struct CharClass {
    negated: bool,
    items: Vec<ClassItem>,
}

#[derive(Debug)]
enum ClassItem {
    Range(char, char),
    Posix(PosixClass),
}

type PosixClass = fn(char) -> bool;

type Continuation<'a> = &'a dyn Fn(usize) -> bool;

impl SegmentPattern {
    pub(super) fn parse(segment: &str) -> Self {
        let chars: Vec<char> = segment.chars().collect();
        let mut parser = Parser { chars: &chars, pos: 0 };
        let nodes = parser.parse_sequence(false);
        let has_extglob = nodes
            .iter()
            .any(|node| matches!(node, Node::Extglob(..)));
        SegmentPattern { nodes, has_extglob }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// minimatch requires a segment that is a lone `*` to match at least one
    /// character, while `*` beside anything else may match none.
    pub(super) fn matches(&self, name: &str) -> bool {
        if matches!(self.nodes.as_slice(), [Node::AnyRun]) && name.is_empty() {
            return false;
        }
        let chars: Vec<char> = name.chars().collect();
        let matcher = Matcher { chars: &chars };
        if self.has_extglob {
            matcher.sequence(&self.nodes, 0, &|end| end == chars.len())
        } else {
            matcher.wildcard(&self.nodes)
        }
    }
}

struct Parser<'a> {
    chars: &'a [char],
    pos: usize,
}

impl Parser<'_> {
    /// Parses until the end, or inside an extglob until an unconsumed `|` or `)`.
    fn parse_sequence(&mut self, in_extglob: bool) -> Vec<Node> {
        let mut nodes = Vec::new();
        while let Some(&current) = self.chars.get(self.pos) {
            if in_extglob && (current == '|' || current == ')') {
                break;
            }
            let node = self.parse_node(current);
            if !(matches!(node, Node::AnyRun) && matches!(nodes.last(), Some(Node::AnyRun))) {
                nodes.push(node);
            }
        }
        nodes
    }

    fn parse_node(&mut self, current: char) -> Node {
        if let Some(kind) = extglob_kind(current)
            && self.chars.get(self.pos + 1) == Some(&'(')
            && let Some(node) = self.try_parse_extglob(kind)
        {
            return node;
        }
        if current == '['
            && let Some(class) = self.try_parse_class()
        {
            return Node::Class(class);
        }
        self.pos += 1;
        match current {
            '*' => Node::AnyRun,
            '?' => Node::AnyChar,
            '\\' => match self.chars.get(self.pos) {
                Some(&escaped) => {
                    self.pos += 1;
                    Node::Literal(escaped)
                }
                None => Node::Literal('\\'),
            },
            literal => Node::Literal(literal),
        }
    }

    /// An unclosed extglob is not one: the parser rewinds and reads it literally.
    fn try_parse_extglob(&mut self, kind: ExtglobKind) -> Option<Node> {
        let start = self.pos;
        self.pos += 2;
        let mut alternatives = Vec::new();
        loop {
            alternatives.push(self.parse_sequence(true));
            match self.chars.get(self.pos) {
                Some('|') => self.pos += 1,
                Some(')') => {
                    self.pos += 1;
                    return Some(Node::Extglob(kind, alternatives));
                }
                _ => {
                    self.pos = start;
                    return None;
                }
            }
        }
    }

    /// An unclosed class is not one: the caller reads its `[` literally.
    fn try_parse_class(&mut self) -> Option<CharClass> {
        let mut pos = self.pos + 1;
        let negated = matches!(self.chars.get(pos), Some('!' | '^'));
        if negated {
            pos += 1;
        }
        let mut items = Vec::new();
        let first = pos;
        loop {
            let &current = self.chars.get(pos)?;
            if current == ']' && pos > first {
                self.pos = pos + 1;
                return Some(CharClass { negated, items });
            }
            let (item, next) = parse_class_item(self.chars, pos)?;
            items.push(item);
            pos = next;
        }
    }
}

fn parse_class_item(chars: &[char], pos: usize) -> Option<(ClassItem, usize)> {
    if chars.get(pos) == Some(&'[')
        && let Some((posix, next)) = parse_posix_class(chars, pos)
    {
        return Some((ClassItem::Posix(posix), next));
    }
    let (low, after_low) = class_char(chars, pos)?;
    let (high, next) = match (chars.get(after_low), chars.get(after_low + 1)) {
        (Some('-'), Some(&after_dash)) if after_dash != ']' => class_char(chars, after_low + 1)?,
        _ => (low, after_low),
    };
    Some((ClassItem::Range(low, high), next))
}

fn extglob_kind(current: char) -> Option<ExtglobKind> {
    match current {
        '@' => Some(ExtglobKind::One),
        '?' => Some(ExtglobKind::ZeroOrOne),
        '+' => Some(ExtglobKind::OneOrMore),
        '*' => Some(ExtglobKind::ZeroOrMore),
        '!' => Some(ExtglobKind::Not),
        _ => None,
    }
}

fn class_char(chars: &[char], pos: usize) -> Option<(char, usize)> {
    match chars.get(pos)? {
        '\\' => chars
            .get(pos + 1)
            .map(|&escaped| (escaped, pos + 2)),
        &literal => Some((literal, pos + 1)),
    }
}

fn parse_posix_class(chars: &[char], pos: usize) -> Option<(PosixClass, usize)> {
    if chars.get(pos + 1) != Some(&':') {
        return None;
    }
    let rest: String = chars[pos + 2..].iter().collect();
    let name_len = rest.find(":]")?;
    let class: PosixClass = match &rest[..name_len] {
        "alnum" => char::is_alphanumeric,
        "alpha" => char::is_alphabetic,
        "ascii" => |current| current.is_ascii(),
        "blank" => |current| current == ' ' || current == '\t',
        "cntrl" => char::is_control,
        "digit" => |current| current.is_ascii_digit(),
        "graph" => |current| !current.is_control() && !current.is_whitespace(),
        "lower" => char::is_lowercase,
        "print" => |current| !current.is_control(),
        "punct" => |current| current.is_ascii_punctuation(),
        "space" => char::is_whitespace,
        "upper" => char::is_uppercase,
        "word" => |current| current.is_alphanumeric() || current == '_',
        "xdigit" => |current| current.is_ascii_hexdigit(),
        _ => return None,
    };
    Some((class, pos + 2 + rest[..name_len].chars().count() + 2))
}

struct Matcher<'a> {
    chars: &'a [char],
}

impl Matcher<'_> {
    /// Whether `nodes` match from `pos` with `rest` accepting where they end.
    fn sequence(&self, nodes: &[Node], pos: usize, rest: Continuation<'_>) -> bool {
        let Some((node, tail)) = nodes.split_first() else {
            return rest(pos);
        };
        let tail_then_rest = |end: usize| self.sequence(tail, end, rest);
        match node {
            Node::AnyRun => (pos..=self.chars.len()).any(tail_then_rest),
            Node::Extglob(kind, alternatives) => {
                self.extglob(*kind, alternatives, pos, &tail_then_rest)
            }
            single => {
                self.char_at(pos)
                    .is_some_and(|current| single.matches_char(current))
                    && tail_then_rest(pos + 1)
            }
        }
    }

    /// Matches nodes without extglobs. Only the latest `*` is ever retried,
    /// so the work stays proportional to the name length times the node
    /// count however many `*`s a rule holds.
    fn wildcard(&self, nodes: &[Node]) -> bool {
        let (mut node_index, mut pos) = (0, 0);
        let mut retry: Option<(usize, usize)> = None;
        while let Some(&current) = self.chars.get(pos) {
            match nodes.get(node_index) {
                Some(Node::AnyRun) => {
                    retry = Some((node_index, pos));
                    node_index += 1;
                    continue;
                }
                Some(node) if node.matches_char(current) => {
                    node_index += 1;
                    pos += 1;
                    continue;
                }
                _ => {}
            }
            let Some((star_index, star_pos)) = retry else { return false };
            retry = Some((star_index, star_pos + 1));
            node_index = star_index + 1;
            pos = star_pos + 1;
        }
        nodes[node_index..]
            .iter()
            .all(|node| matches!(node, Node::AnyRun))
    }

    /// `!(…)` follows minimatch: it fails when an alternative followed by the
    /// rest of the segment matches here, and otherwise matches any run.
    fn extglob(
        &self,
        kind: ExtglobKind,
        alternatives: &[Vec<Node>],
        pos: usize,
        rest: Continuation<'_>,
    ) -> bool {
        match kind {
            ExtglobKind::One => self.any_alternative(alternatives, pos, rest),
            ExtglobKind::ZeroOrOne => rest(pos) || self.any_alternative(alternatives, pos, rest),
            ExtglobKind::OneOrMore => self.repeat(alternatives, pos, rest),
            ExtglobKind::ZeroOrMore => rest(pos) || self.repeat(alternatives, pos, rest),
            ExtglobKind::Not => {
                !self.any_alternative(alternatives, pos, rest)
                    && (pos..=self.chars.len()).any(rest)
            }
        }
    }

    fn any_alternative(
        &self,
        alternatives: &[Vec<Node>],
        pos: usize,
        rest: Continuation<'_>,
    ) -> bool {
        alternatives
            .iter()
            .any(|alternative| self.sequence(alternative, pos, rest))
    }

    /// One or more alternatives in a row. A repetition must consume input, so
    /// an alternative that matches the empty string cannot loop forever.
    fn repeat(&self, alternatives: &[Vec<Node>], pos: usize, rest: Continuation<'_>) -> bool {
        self.any_alternative(alternatives, pos, &|end| {
            rest(end) || (end > pos && self.repeat(alternatives, end, rest))
        })
    }

    fn char_at(&self, pos: usize) -> Option<char> {
        self.chars.get(pos).copied()
    }
}

impl Node {
    /// Whether a node that consumes exactly one character accepts `current`.
    fn matches_char(&self, current: char) -> bool {
        match self {
            Node::Literal(literal) => chars_eq_ignoring_case(current, *literal),
            Node::AnyChar => true,
            Node::Class(class) => class.matches(current),
            Node::AnyRun | Node::Extglob(..) => false,
        }
    }
}

impl CharClass {
    fn matches(&self, current: char) -> bool {
        let lower = current
            .to_lowercase()
            .next()
            .unwrap_or(current);
        let upper = current
            .to_uppercase()
            .next()
            .unwrap_or(current);
        let found = self.items
            .iter()
            .any(|item| match item {
                ClassItem::Range(low, high) => [current, lower, upper]
                    .iter()
                    .any(|candidate| (low..=high).contains(&candidate)),
                ClassItem::Posix(class) => class(current),
            });
        found != self.negated
    }
}

fn chars_eq_ignoring_case(left: char, right: char) -> bool {
    left == right || left.to_lowercase().eq(right.to_lowercase())
}
