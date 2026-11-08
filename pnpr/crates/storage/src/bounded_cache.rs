use std::{
    collections::{HashMap, VecDeque},
    hash::Hash,
};

/// A map that drops its oldest entries once their weights exceed a budget.
#[derive(Debug)]
pub struct BoundedCache<Key, Value> {
    entries: HashMap<Key, Entry<Value>>,
    /// Keys in insertion order, with the insertion that put them there.
    order: VecDeque<(Key, u64)>,
    weight: usize,
    insertions: u64,
}

#[derive(Debug)]
struct Entry<Value> {
    value: Value,
    weight: usize,
    insertion: u64,
}

impl<Key, Value> Default for BoundedCache<Key, Value> {
    fn default() -> Self {
        Self { entries: HashMap::new(), order: VecDeque::new(), weight: 0, insertions: 0 }
    }
}

impl<Key: Clone + Eq + Hash, Value> BoundedCache<Key, Value> {
    /// The value stored for `key`.
    pub fn get(&self, key: &Key) -> Option<&Value> {
        self.entries.get(key).map(|entry| &entry.value)
    }

    /// Insert an entry no heavier than `budget`, evicting oldest entries to fit.
    pub fn insert(&mut self, key: Key, value: Value, weight: usize, budget: usize) {
        if weight > budget {
            return;
        }
        self.insertions += 1;
        let entry = Entry { value, weight, insertion: self.insertions };
        if let Some(replaced) = self.entries.insert(key.clone(), entry) {
            self.weight -= replaced.weight;
        }
        self.weight += weight;
        self.order.push_back((key, self.insertions));
        while self.weight > budget {
            let Some((oldest, insertion)) = self.order.pop_front() else { break };
            if self.entries
                .get(&oldest)
                .is_some_and(|entry| entry.insertion == insertion)
                && let Some(evicted) = self.entries.remove(&oldest)
            {
                self.weight -= evicted.weight;
            }
        }
        // A replaced entry leaves its old position behind; drop those before
        // they outnumber the live ones.
        if self.order.len() > 2 * self.entries.len() + 64 {
            let entries = &self.entries;
            self.order.retain(|(key, insertion)| {
                entries
                    .get(key)
                    .is_some_and(|entry| entry.insertion == *insertion)
            });
        }
    }
}

#[cfg(test)]
#[path = "bounded_cache_tests.rs"]
mod tests;
