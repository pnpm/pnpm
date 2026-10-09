//! One `/`-free segment of an ignore rule: minimatch's wildcards, character
//! classes, and extglobs, matched case-insensitively.

use std::cell::Cell;

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

/// Extglobs nested deeper than this are read literally, so a crafted rule
/// cannot exhaust the stack while it is parsed.
const MAX_EXTGLOB_NESTING: usize = 8;

/// The work an extglob match may take before the name counts as not
/// matching. Ignore files of git dependencies are untrusted, and extglob
/// backtracking is otherwise exponential in the worst case.
const MAX_MATCH_STEPS: usize = 1_000_000;

/// The nesting of backtracking calls a match may reach, for the same reason.
const MAX_MATCH_DEPTH: usize = 256;

impl SegmentPattern {
    pub(super) fn parse(segment: &str) -> Self {
        let chars: Vec<char> = segment.chars().collect();
        let mut parser = Parser { chars: &chars, pos: 0, nesting: 0 };
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
        let matcher = Matcher {
            chars: &chars,
            steps: Cell::new(0),
            depth: Cell::new(0),
            exhausted: Cell::new(false),
        };
        if !self.has_extglob {
            return matcher.wildcard(&self.nodes);
        }
        let matched = matcher.sequence(&self.nodes, 0, &|end| end == chars.len());
        matched && !matcher.exhausted.get()
    }
}

struct Parser<'a> {
    chars: &'a [char],
    pos: usize,
    /// How many extglobs enclose the current position.
    nesting: usize,
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
            && self.nesting < MAX_EXTGLOB_NESTING
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
        self.nesting += 1;
        let alternatives = self.parse_alternatives();
        self.nesting -= 1;
        if alternatives.is_none() {
            self.pos = start;
        }
        alternatives.map(|alternatives| Node::Extglob(kind, alternatives))
    }

    /// The `|`-separated alternatives up to and including the closing `)`.
    fn parse_alternatives(&mut self) -> Option<Vec<Vec<Node>>> {
        let mut alternatives = Vec::new();
        loop {
            alternatives.push(self.parse_sequence(true));
            match self.chars.get(self.pos)? {
                '|' => self.pos += 1,
                ')' => {
                    self.pos += 1;
                    return Some(alternatives);
                }
                _ => unreachable!("parse_sequence stops only at `|`, `)`, or the end"),
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
    steps: Cell<usize>,
    depth: Cell<usize>,
    /// Set once the match exceeds [`MAX_MATCH_STEPS`] or [`MAX_MATCH_DEPTH`].
    exhausted: Cell<bool>,
}

impl Matcher<'_> {
    /// Whether `nodes` match from `pos` with `rest` accepting where they end.
    fn sequence(&self, nodes: &[Node], pos: usize, rest: Continuation<'_>) -> bool {
        let steps = self.steps.get() + 1;
        self.steps.set(steps);
        if steps > MAX_MATCH_STEPS || self.depth.get() >= MAX_MATCH_DEPTH {
            self.exhausted.set(true);
        }
        if self.exhausted.get() {
            return false;
        }
        self.depth.set(self.depth.get() + 1);
        let matched = self.backtrack(nodes, pos, rest);
        self.depth.set(self.depth.get() - 1);
        matched
    }

    /// Consumes leading single-character nodes in a loop, so only `*` and
    /// extglobs nest calls.
    fn backtrack(&self, nodes: &[Node], mut pos: usize, rest: Continuation<'_>) -> bool {
        let mut remaining = nodes;
        while let Some((node, tail)) = remaining.split_first() {
            let tail_then_rest = |end: usize| self.sequence(tail, end, rest);
            match node {
                Node::AnyRun => return (pos..=self.chars.len()).any(tail_then_rest),
                Node::Extglob(kind, alternatives) => {
                    return self.extglob(*kind, alternatives, pos, &tail_then_rest);
                }
                single
                    if self
                        .char_at(pos)
                        .is_some_and(|current| single.matches_char(current)) =>
                {
                    pos += 1;
                    remaining = tail;
                }
                _ => return false,
            }
        }
        rest(pos)
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
