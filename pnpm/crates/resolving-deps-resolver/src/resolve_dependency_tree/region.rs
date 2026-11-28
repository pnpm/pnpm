//! Whether every package a root reaches is fit, by a check on each
//! package alone that the caller supplies, decided for the root and for
//! every package found on the way.
//!
//! Recursing once per edge overflows the stack of a thread on a long
//! enough dependency chain, so [`decide`] keeps a stack of its own and
//! leaves the answers to `pnpm_reachability::reaching`, which Creusot
//! proves exact.

use pnpm_reachability::reaching;
use rustc_hash::FxHashMap as HashMap;
use std::hash::Hash;

/// What [`decide`] learns about one package by inspecting it alone.
pub(super) enum Inspected<Key> {
    /// Its answer does not wait on its children: the caller has the answer
    /// memoised, or the package itself is unfit and the answer is `false`.
    Settled(bool),
    /// It is fit itself, so its answer is whether every package these
    /// children reach is fit.
    Children(Vec<Key>),
}

/// Every package reachable from `root` through packages `inspect` does not
/// settle, `root` first, each with its answer: `true` when no package it
/// reaches is settled `false`. `inspect` is called once per package, and
/// the children of a settled package are not visited.
///
/// The members of a cycle reach each other, so they share one answer.
pub(super) fn decide<Key>(
    root: &Key,
    mut inspect: impl FnMut(&Key) -> Inspected<Key>,
) -> Vec<(Key, bool)>
where
    Key: Clone + Eq + Hash,
{
    let mut region = Region {
        keys: Vec::new(),
        index_of: HashMap::default(),
        settled: Vec::new(),
        children: Vec::new(),
    };
    let mut pending = vec![region.intern(root)];
    while let Some(node) = pending.pop() {
        let children = match inspect(&region.keys[node]) {
            Inspected::Settled(answer) => {
                region.settled[node] = Some(answer);
                continue;
            }
            Inspected::Children(children) => children,
        };
        for child in &children {
            let before = region.keys.len();
            let child = region.intern(child);
            if child == before {
                pending.push(child);
            }
            region.children[node].push(child);
        }
    }
    region.answers()
}

/// The packages [`decide`] has found, indexed in the order it found them.
/// A settled package has no children here, since its own are not visited.
struct Region<Key> {
    keys: Vec<Key>,
    index_of: HashMap<Key, usize>,
    settled: Vec<Option<bool>>,
    children: Vec<Vec<usize>>,
}

impl<Key> Region<Key>
where
    Key: Clone + Eq + Hash,
{
    /// `key`'s index, which is the next free one when `key` is new.
    fn intern(&mut self, key: &Key) -> usize {
        if let Some(&index) = self.index_of.get(key) {
            return index;
        }
        let index = self.keys.len();
        self.index_of.insert(key.clone(), index);
        self.keys.push(key.clone());
        self.settled.push(None);
        self.children.push(Vec::new());
        index
    }

    fn answers(self) -> Vec<(Key, bool)> {
        let unfit: Vec<bool> = self.settled
            .iter()
            .map(|settled| !settled.unwrap_or(true))
            .collect();
        let reaches_unfit = reaching(&self.children, &unfit);
        self.keys
            .into_iter()
            .zip(reaches_unfit)
            .map(|(key, reaches_unfit)| (key, !reaches_unfit))
            .collect()
    }
}

#[cfg(test)]
mod tests;
