//! Functions in different top-level modules that access the same fields.

use crate::program::{
    Function,
    Program,
};
use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    fmt::Write as _,
};

/// Two functions that overlap, lower index first, and the fields both access.
type Pairs = BTreeMap<(usize, usize), BTreeSet<usize>>;

/// Functions joined by overlaps, directly or through other members.
#[derive(Debug, PartialEq, Eq)]
pub struct Group {
    /// Indexes into [`Program::functions`], in the order of their locations.
    pub functions: Vec<usize>,
    /// The most fields two of the functions share.
    pub shared: usize,
    /// The fields two of the functions share, as indexes into
    /// [`Program::fields`].
    pub fields: BTreeSet<usize>,
}

/// The groups of overlapping functions, the largest overlap first. Two
/// functions overlap when:
///
/// - they are in different top-level modules;
/// - both access at least `min_shared` of the same fields;
/// - those are at least half of the fields that either one accesses;
/// - neither reaches the other through calls.
pub fn find(program: &Program, min_shared: usize) -> Vec<Group> {
    let pairs = overlapping_pairs(program, min_shared);
    let mut groups = components(&pairs)
        .into_iter()
        .map(|members| Group::new(program, members, &pairs))
        .collect::<Vec<_>>();
    groups.sort_by_key(|group| {
        let first = group.functions
            .first()
            .map(|function| program.functions[*function].order());
        (std::cmp::Reverse(group.shared), std::cmp::Reverse(group.functions.len()), first)
    });
    groups
}

impl Group {
    fn new(program: &Program, members: BTreeSet<usize>, pairs: &Pairs) -> Self {
        let inside = pairs
            .iter()
            .filter(|((first, _), _)| members.contains(first))
            .map(|(_, shared)| shared);
        let shared = inside
            .clone()
            .map(BTreeSet::len)
            .max()
            .unwrap_or_default();
        let fields = inside.flatten().copied().collect();
        let mut functions = members.into_iter().collect::<Vec<_>>();
        functions.sort_by_key(|function| program.functions[*function].order());
        Self { functions, shared, fields }
    }
}

fn overlapping_pairs(program: &Program, min_shared: usize) -> Pairs {
    let mut pairs = Pairs::new();
    for (first, second) in candidates(program, min_shared) {
        let (left, right) = (&program.functions[first], &program.functions[second]);
        let Some(shared) = shared_fields(left, right, min_shared) else { continue };
        if !reaches(program, first, second) && !reaches(program, second, first) {
            pairs.insert((first, second), shared);
        }
    }
    pairs
}

/// The pairs of functions in different places that access a common field
/// and at least `min_shared` fields each.
fn candidates(program: &Program, min_shared: usize) -> BTreeSet<(usize, usize)> {
    let mut accessors = BTreeMap::<usize, Vec<usize>>::new();
    let eligible = program.functions
        .iter()
        .enumerate()
        .filter(|(_, function)| function.accessed.len() >= min_shared.max(1));
    for (index, function) in eligible {
        for field in &function.accessed {
            accessors
                .entry(*field)
                .or_default()
                .push(index);
        }
    }
    let mut candidates = BTreeSet::new();
    for functions in accessors.values() {
        for (position, first) in functions.iter().enumerate() {
            let others = functions[position + 1..]
                .iter()
                .filter(|second| {
                    program.functions[**second].place != program.functions[*first].place
                });
            candidates.extend(others.map(|second| (*first, *second)));
        }
    }
    candidates
}

/// The fields both functions access, when they are enough to overlap.
fn shared_fields(left: &Function, right: &Function, min_shared: usize) -> Option<BTreeSet<usize>> {
    let shared = left.accessed
        .intersection(&right.accessed)
        .copied()
        .collect::<BTreeSet<_>>();
    let either = left.accessed.len() + right.accessed.len() - shared.len();
    (shared.len() >= min_shared && 2 * shared.len() >= either).then_some(shared)
}

/// Whether `from` calls `to`, directly or through other functions.
fn reaches(program: &Program, from: usize, to: usize) -> bool {
    let mut seen = BTreeSet::from([from]);
    let mut pending = vec![from];
    while let Some(function) = pending.pop() {
        let callees = &program.functions[function].called;
        if callees.contains(&to) {
            return true;
        }
        pending.extend(
            callees
                .iter()
                .filter(|callee| seen.insert(**callee)),
        );
    }
    false
}

/// The connected components of the graph whose edges are `pairs`.
fn components(pairs: &Pairs) -> Vec<BTreeSet<usize>> {
    let mut neighbors = BTreeMap::<usize, Vec<usize>>::new();
    for (first, second) in pairs.keys() {
        neighbors
            .entry(*first)
            .or_default()
            .push(*second);
        neighbors
            .entry(*second)
            .or_default()
            .push(*first);
    }
    let mut seen = BTreeSet::new();
    let mut components = Vec::new();
    for start in neighbors.keys() {
        if seen.insert(*start) {
            components.push(component(*start, &neighbors, &mut seen));
        }
    }
    components
}

/// The functions `start` reaches through `neighbors`, itself included.
fn component(
    start: usize,
    neighbors: &BTreeMap<usize, Vec<usize>>,
    seen: &mut BTreeSet<usize>,
) -> BTreeSet<usize> {
    let mut component = BTreeSet::from([start]);
    let mut pending = vec![start];
    while let Some(function) = pending.pop() {
        let unseen = neighbors[&function]
            .iter()
            .copied()
            .filter(|neighbor| seen.insert(*neighbor))
            .collect::<Vec<_>>();
        component.extend(&unseen);
        pending.extend(unseen);
    }
    component
}

/// Each group: its size, its largest overlap, how many of the shared
/// fields each type owns, and the location of each function.
pub fn render(program: &Program, groups: &[Group]) -> String {
    let mut text = String::new();
    for group in groups {
        let size = group.functions.len();
        let heading = if size == 2 {
            format!("2 functions share {} fields", group.shared)
        } else {
            format!("{size} functions, two of which share {} fields", group.shared)
        };
        let _ = writeln!(text, "{heading}: {}", field_owners(program, &group.fields));
        for function in &group.functions {
            let _ = writeln!(text, "  {}", program.functions[*function].location());
        }
    }
    text
}

/// `Type (count)` for each type that owns some of `fields`, most fields first.
fn field_owners(program: &Program, fields: &BTreeSet<usize>) -> String {
    let mut counts = BTreeMap::<&str, usize>::new();
    for field in fields {
        let owner = &program.types[program.fields[*field].owner];
        *counts.entry(owner).or_default() += 1;
    }
    let mut owners = counts.into_iter().collect::<Vec<_>>();
    owners.sort_by_key(|(owner, count)| (std::cmp::Reverse(*count), *owner));
    owners
        .into_iter()
        .map(|(owner, count)| format!("{owner} ({count})"))
        .collect::<Vec<_>>()
        .join(", ")
}
