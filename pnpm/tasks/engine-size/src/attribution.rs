//! Approximate attribution of machine code to crates and functions.
//!
//! The probe's symbol table names every function and its size. The crate
//! of a function is the first segment of its demangled path, or of the
//! self type of a trait method. Generic code is attributed to the crate
//! that defines it, not to the crate that instantiated it: a
//! `serde::de::Visitor` method monomorphized for a pnpm type counts as
//! `serde` unless the visitor type itself lives in pnpm. These are
//! symbol-based estimates, not independently removable bytes.

use object::{Object, ObjectSection, ObjectSymbol, SectionIndex, SymbolKind};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

/// The bucket for symbols that are not Rust, such as C libraries.
const NON_RUST: &str = "[non-Rust]";

#[derive(Debug, Serialize)]
pub struct Attribution {
    /// Code bytes covered by the function symbols.
    pub code_size: u64,
    /// Code bytes of the crates whose name starts with `pnpm_`.
    pub pnpm_code_size: u64,
    /// The largest crates.
    pub crates: Vec<Entry>,
    /// The largest functions. Instances of a generic function share a
    /// demangled name and are summed into one entry.
    pub functions: Vec<Entry>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub name: String,
    pub size: u64,
    /// Number of symbols summed into this entry.
    pub symbols: u64,
}

/// A defined function symbol: its address and the size its symbol table
/// entry declares, which is `0` where the format records none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeSymbol {
    pub address: u64,
    pub size: u64,
    pub name: String,
}

pub fn attribute(binary: &[u8], top: usize) -> Result<Attribution, String> {
    let file = object::File::parse(binary).map_err(|error| format!("parse the probe: {error}"))?;
    let symbols = sized_code_symbols(&file);
    if symbols.is_empty() {
        return Err("the probe has no function symbols to attribute".to_string());
    }
    let mut crates = BTreeMap::<String, (u64, u64)>::new();
    let mut functions = HashMap::<String, (u64, u64)>::new();
    for symbol in &symbols {
        let (crate_name, function) = demangle(&symbol.name);
        add(crates.entry(crate_name).or_default(), symbol.size);
        add(functions.entry(function).or_default(), symbol.size);
    }
    let pnpm_code_size = crates
        .iter()
        .filter(|(name, _)| name.starts_with("pnpm_"))
        .map(|(_, (size, _))| size)
        .sum();
    Ok(Attribution {
        code_size: symbols
            .iter()
            .map(|symbol| symbol.size)
            .sum(),
        pnpm_code_size,
        crates: largest(crates, top),
        functions: largest(functions, top),
    })
}

fn add(total: &mut (u64, u64), size: u64) {
    total.0 += size;
    total.1 += 1;
}

fn largest(totals: impl IntoIterator<Item = (String, (u64, u64))>, top: usize) -> Vec<Entry> {
    let mut entries: Vec<Entry> = totals
        .into_iter()
        .map(|(name, (size, symbols))| Entry { name, size, symbols })
        .collect();
    entries.sort_by(|a, b| {
        b.size
            .cmp(&a.size)
            .then_with(|| a.name.cmp(&b.name))
    });
    entries.truncate(top);
    entries
}

/// The function symbols of every code section, sized by [`fill_sizes`].
fn sized_code_symbols(file: &object::File<'_>) -> Vec<CodeSymbol> {
    let mut by_section = HashMap::<SectionIndex, Vec<CodeSymbol>>::new();
    for symbol in file.symbols() {
        let (Some(index), Ok(name)) = (symbol.section_index(), symbol.name()) else { continue };
        if symbol.kind() == SymbolKind::Text && symbol.is_definition() {
            let code_symbol = CodeSymbol {
                address: symbol.address(),
                size: symbol.size(),
                name: name.to_string(),
            };
            by_section
                .entry(index)
                .or_default()
                .push(code_symbol);
        }
    }
    by_section
        .into_iter()
        .filter_map(|(index, symbols)| {
            let section = file.section_by_index(index).ok()?;
            Some(fill_sizes(symbols, section.address() + section.size()))
        })
        .flatten()
        .collect()
}

/// Sort `symbols` by address, keep one symbol per address, and give a
/// symbol without a declared size the distance to the next symbol or to
/// `section_end`. Mach-O records no symbol sizes, so every size there
/// comes from this.
pub fn fill_sizes(mut symbols: Vec<CodeSymbol>, section_end: u64) -> Vec<CodeSymbol> {
    symbols.sort_by(|a, b| {
        a.address
            .cmp(&b.address)
            .then_with(|| b.size.cmp(&a.size))
    });
    symbols.dedup_by_key(|symbol| symbol.address);
    let next_addresses: Vec<u64> = symbols
        .iter()
        .skip(1)
        .map(|symbol| symbol.address)
        .chain([section_end])
        .collect();
    for (symbol, next_address) in symbols.iter_mut().zip(next_addresses) {
        if symbol.size == 0 {
            symbol.size = next_address.saturating_sub(symbol.address);
        }
    }
    symbols
}

/// The crate and the demangled path, without its hash, of `symbol`.
fn demangle(symbol: &str) -> (String, String) {
    // Mach-O prefixes every symbol with an underscore.
    let rust_symbol = rustc_demangle::try_demangle(symbol)
        .or_else(|_| rustc_demangle::try_demangle(symbol.strip_prefix('_').unwrap_or(symbol)));
    match rust_symbol {
        Ok(demangled) => {
            let path = format!("{demangled:#}");
            (crate_of_path(&path).to_string(), path)
        }
        Err(_) => (NON_RUST.to_string(), symbol.to_string()),
    }
}

/// The first path segment of a demangled Rust path. For a qualified path
/// such as `<&mut a::T as b::Trait>::f`, the segment of its self type.
pub fn crate_of_path(path: &str) -> &str {
    let mut rest = path;
    loop {
        let trimmed = rest
            .trim_start_matches(['<', '&', '*', '(', '['])
            .trim_start_matches("mut ")
            .trim_start_matches("const ")
            .trim_start_matches("dyn ");
        if trimmed.len() == rest.len() {
            break;
        }
        rest = trimmed;
    }
    let end = rest
        .find(|char: char| !(char.is_alphanumeric() || char == '_'))
        .unwrap_or(rest.len());
    match &rest[..end] {
        "" => path,
        segment => segment,
    }
}

#[cfg(test)]
mod tests;
