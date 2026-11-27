//! The types whose fields functions in several top-level modules access.

use crate::program::{
    Place,
    Program,
};
use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    fmt::Write as _,
};

/// Where the fields of one type are accessed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TypeUse {
    pub places: BTreeMap<Place, PlaceUse>,
}

/// The accesses to one type's fields from one place.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PlaceUse {
    /// Indexes into [`Program::fields`].
    pub fields: BTreeSet<usize>,
    /// How many of the type's fields each function accesses, by its index
    /// into [`Program::functions`].
    pub functions: BTreeMap<usize, usize>,
}

impl TypeUse {
    /// The most fields that two places both access.
    pub fn shared(&self) -> usize {
        let accessed = self.places
            .values()
            .map(|place_use| &place_use.fields)
            .collect::<Vec<_>>();
        let pairs = accessed
            .iter()
            .enumerate()
            .flat_map(|(position, first)| {
                accessed[position + 1..]
                    .iter()
                    .map(move |second| (first, second))
            });
        pairs
            .map(|(first, second)| first.intersection(second).count())
            .max()
            .unwrap_or_default()
    }
}

/// The use of each type, in the order of [`Program::types`].
pub fn find(program: &Program) -> Vec<TypeUse> {
    let mut uses = Vec::new();
    uses.resize_with(program.types.len(), TypeUse::default);
    for (index, function) in program.functions.iter().enumerate() {
        for field in &function.accessed {
            let type_use: &mut TypeUse = &mut uses[program.fields[*field].owner];
            let place_use = type_use.places.entry(function.place.clone()).or_default();
            place_use.fields.insert(*field);
            *place_use.functions.entry(index).or_default() += 1;
        }
    }
    uses
}

/// One line per type with at least `min_shared` fields that two places both
/// access, the most such fields first.
pub fn render_summary(program: &Program, uses: &[TypeUse], min_shared: usize) -> String {
    let mut rows = program.types
        .iter()
        .zip(uses)
        .map(|(type_path, type_use)| (type_use.shared(), type_use.places.len(), type_path))
        .filter(|(shared, ..)| *shared >= min_shared.max(1))
        .collect::<Vec<_>>();
    rows.sort_by_key(|(shared, places, type_path)| {
        (std::cmp::Reverse(*shared), *places, *type_path)
    });
    let mut text = String::from("type | fields two modules both access | modules\n");
    for (shared, places, type_path) in rows {
        let _ = writeln!(text, "{type_path} | {shared} | {places}");
    }
    text
}

/// The places that access the fields of each type whose path is
/// `type_path`, the most fields first, each with its functions and how many
/// of the fields each one accesses. `None` when no type has that path.
pub fn render_type(program: &Program, uses: &[TypeUse], type_path: &str) -> Option<String> {
    let mut named = program.types
        .iter()
        .zip(uses)
        .filter(|(path, _)| *path == type_path)
        .map(|(_, type_use)| type_use)
        .peekable();
    named.peek()?;
    let mut text = String::new();
    for type_use in named {
        let fields = type_use.places
            .values()
            .flat_map(|place_use| &place_use.fields)
            .collect::<BTreeSet<_>>();
        let _ = writeln!(
            text,
            "{type_path}: {} fields accessed in {} modules",
            fields.len(),
            type_use.places.len(),
        );
        push_places(&mut text, program, type_use);
    }
    Some(text)
}

fn push_places(text: &mut String, program: &Program, type_use: &TypeUse) {
    let mut places = type_use.places.iter().collect::<Vec<_>>();
    places.sort_by_key(|(place, place_use)| (std::cmp::Reverse(place_use.fields.len()), *place));
    for (place, place_use) in places {
        let _ = writeln!(
            text,
            "{place}: {} fields in {} functions",
            place_use.fields.len(),
            place_use.functions.len(),
        );
        let mut functions = place_use.functions
            .iter()
            .map(|(function, count)| (&program.functions[*function], *count))
            .collect::<Vec<_>>();
        functions.sort_by_key(|(function, count)| (std::cmp::Reverse(*count), function.order()));
        for (function, count) in functions {
            let _ = writeln!(text, "  {} ({count})", function.location());
        }
    }
}
