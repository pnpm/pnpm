//! Which modules and items of a crate are reachable from outside it: through
//! a chain of `pub mod` from the crate root, or through a `pub use`, globs
//! included, in a module that is itself reachable.

use crate::{
    modules::{CrateModules, ModulePath},
    surface::{Exposure, Surface},
    use_tree::UsePath,
};
use std::collections::{BTreeMap, BTreeSet};

/// A module of the crate, read from its own file or inline in another.
pub struct Scope<'surface> {
    /// Whether the parent declares it `pub mod`.
    pub is_public: bool,
    pub surface: &'surface Surface,
}

/// Every module of a crate, inline modules included, by path.
pub fn scopes(modules: &CrateModules) -> BTreeMap<ModulePath, Scope<'_>> {
    let mut scopes = BTreeMap::new();
    for (path, is_public, surface) in modules.surfaces() {
        insert_scope(&mut scopes, path.clone(), Scope { is_public, surface });
    }
    scopes
}

fn insert_scope<'surface>(
    scopes: &mut BTreeMap<ModulePath, Scope<'surface>>,
    path: ModulePath,
    scope: Scope<'surface>,
) {
    for inline in &scope.surface.inline_modules {
        let mut inline_path = path.clone();
        inline_path.push(inline.name.clone());
        let inline_scope = Scope { is_public: inline.is_public, surface: &inline.surface };
        insert_scope(scopes, inline_path, inline_scope);
    }
    scopes.insert(path, scope);
}

/// What a `pub use` or `pub mod` makes reachable.
enum Export {
    /// Every `pub` item of the module.
    Module(ModulePath),
    /// One item of the module, by name.
    Item(ModulePath, String),
}

/// The modules and items reachable from outside the crate.
#[derive(Debug, Default)]
pub struct Exported {
    modules: BTreeSet<ModulePath>,
    items: BTreeSet<(ModulePath, String)>,
}

impl Exported {
    pub fn find(scopes: &BTreeMap<ModulePath, Scope<'_>>) -> Self {
        let mut exported = Self::default();
        let mut queue = vec![Export::Module(ModulePath::new())];
        while let Some(export) = queue.pop() {
            match export {
                Export::Module(path) => exported.add_module(scopes, &path, &mut queue),
                Export::Item(path, name) => exported.add_item(scopes, &path, &name, &mut queue),
            }
        }
        exported
    }

    fn add_module(
        &mut self,
        scopes: &BTreeMap<ModulePath, Scope<'_>>,
        path: &ModulePath,
        queue: &mut Vec<Export>,
    ) {
        let Some(scope) = scopes.get(path) else { return };
        if !self.modules.insert(path.clone()) {
            return;
        }
        let public_children = scopes
            .range(path.clone()..)
            .take_while(|(child, _)| child.starts_with(path))
            .filter(|(child, scope)| child.len() == path.len() + 1 && scope.is_public)
            .map(|(child, _)| Export::Module(child.clone()));
        queue.extend(public_children);
        let reexports = scope.surface.reexported.iter();
        queue.extend(reexports.filter_map(|reexport| target(scopes, path, reexport)));
    }

    /// Adds the item `name` of `path`. A module re-exported by name makes its
    /// own `pub` items reachable, and a `pub use` that binds `name` passes the
    /// export on to what it imports.
    fn add_item(
        &mut self,
        scopes: &BTreeMap<ModulePath, Scope<'_>>,
        path: &ModulePath,
        name: &str,
        queue: &mut Vec<Export>,
    ) {
        let Some(scope) = scopes.get(path) else { return };
        if !self.items.insert((path.clone(), name.to_owned())) {
            return;
        }
        let mut child = path.clone();
        child.push(name.to_owned());
        queue.push(Export::Module(child));
        let binds = |reexport: &&UsePath| reexport.binding.as_deref() == Some(name);
        let reexports = scope.surface.reexported.iter().filter(binds);
        queue.extend(reexports.filter_map(|reexport| target(scopes, path, reexport)));
    }

    /// Whether the signature `exposure`, read in `module`, is reachable: its
    /// owner is an exported item, or it has no owner and the module is
    /// exported.
    pub fn exposes(
        &self,
        scopes: &BTreeMap<ModulePath, Scope<'_>>,
        module: &ModulePath,
        exposure: &Exposure,
    ) -> bool {
        let Some((name, prefix)) = exposure.owner.split_last() else {
            return self.modules.contains(module);
        };
        let Some(owner_module) = resolve_module(scopes, module, prefix) else {
            return self.modules.contains(module);
        };
        let is_public_item = || {
            scopes
                .get(&owner_module)
                .is_some_and(|scope| scope.surface.public_items.contains(name))
        };
        self.items.contains(&(owner_module.clone(), name.clone()))
            || self.modules.contains(&owner_module) && is_public_item()
    }

    /// Whether `reexport`, a `pub use` in `module`, is reachable from outside
    /// the crate.
    pub fn reexports(&self, module: &ModulePath, reexport: &UsePath) -> bool {
        let named = || {
            reexport.binding
                .as_ref()
                .is_some_and(|name| self.items.contains(&(module.clone(), name.clone())))
        };
        self.modules.contains(module) || named()
    }
}

/// What the `pub use` `reexport` in `from` makes reachable, if it points into
/// the crate.
fn target(
    scopes: &BTreeMap<ModulePath, Scope<'_>>,
    from: &ModulePath,
    reexport: &UsePath,
) -> Option<Export> {
    if reexport.binding.is_none() {
        return resolve_module(scopes, from, &reexport.segments).map(Export::Module);
    }
    let (name, prefix) = reexport.segments.split_last()?;
    let module = resolve_module(scopes, from, prefix)?;
    Some(Export::Item(module, name.clone()))
}

/// The module of the crate that `segments`, written in `from`, names, or
/// `None` when they lead out of the crate.
fn resolve_module(
    scopes: &BTreeMap<ModulePath, Scope<'_>>,
    from: &ModulePath,
    segments: &[String],
) -> Option<ModulePath> {
    let supers = segments
        .iter()
        .take_while(|segment| *segment == "super")
        .count();
    let (mut module, rest) = match segments.first().map(String::as_str) {
        Some("crate") => (ModulePath::new(), &segments[1..]),
        Some("self") => (from.clone(), &segments[1..]),
        _ if supers > 0 => {
            let kept = from.len().checked_sub(supers)?;
            (from[..kept].to_vec(), &segments[supers..])
        }
        _ => (from.clone(), segments),
    };
    for segment in rest {
        module.push(segment.clone());
        if !scopes.contains_key(&module) {
            return None;
        }
    }
    Some(module)
}
