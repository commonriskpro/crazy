// ── ail-stdlib::collections ───────────────────────────────────────────────
//
// Collection types for the AIL `std.collections` module.
//
// # Contracts (from docs/stdlib.md)
//
// - length >= 0
// - Set has no duplicates
// - Map keys unique
// - order explicit by type

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

// ── List ──────────────────────────────────────────────────────────────────

/// An ordered, growable list of elements.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct List<T>(pub Vec<T>);

impl<T> List<T> {
    pub fn new() -> Self {
        Self(Vec::new())
    }
    pub fn from_vec(v: Vec<T>) -> Self {
        Self(v)
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn push(&mut self, item: T) {
        self.0.push(item);
    }
    pub fn get(&self, index: usize) -> Option<&T> {
        self.0.get(index)
    }
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.0.iter()
    }
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }
}

impl<T: Clone> List<T> {
    pub fn concat(&self, other: &List<T>) -> List<T> {
        let mut v = self.0.clone();
        v.extend_from_slice(&other.0);
        List(v)
    }
}

// ── Set ───────────────────────────────────────────────────────────────────

/// An unordered set with no duplicates (hash-based).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Set<T: std::hash::Hash + Eq>(pub HashSet<T>);

impl<T: std::hash::Hash + Eq> Set<T> {
    pub fn new() -> Self {
        Self(HashSet::new())
    }
    pub fn insert(&mut self, item: T) -> bool {
        self.0.insert(item)
    }
    pub fn contains(&self, item: &T) -> bool {
        self.0.contains(item)
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn remove(&mut self, item: &T) -> bool {
        self.0.remove(item)
    }
}

/// An ordered set with no duplicates (B-tree based; requires `Ord`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct OrderedSet<T: Ord>(pub BTreeSet<T>);

impl<T: Ord> OrderedSet<T> {
    pub fn new() -> Self {
        Self(BTreeSet::new())
    }
    pub fn insert(&mut self, item: T) -> bool {
        self.0.insert(item)
    }
    pub fn contains(&self, item: &T) -> bool {
        self.0.contains(item)
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

// ── Map ───────────────────────────────────────────────────────────────────

/// An unordered map with unique keys (hash-based).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Map<K: std::hash::Hash + Eq, V>(pub HashMap<K, V>);

impl<K: std::hash::Hash + Eq, V> Map<K, V> {
    pub fn new() -> Self {
        Self(HashMap::new())
    }
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.0.insert(key, value)
    }
    pub fn get(&self, key: &K) -> Option<&V> {
        self.0.get(key)
    }
    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.0.remove(key)
    }
    pub fn contains_key(&self, key: &K) -> bool {
        self.0.contains_key(key)
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// An ordered map with unique keys (B-tree based; requires `Ord` on keys).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct OrderedMap<K: Ord, V>(pub BTreeMap<K, V>);

impl<K: Ord, V> OrderedMap<K, V> {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.0.insert(key, value)
    }
    pub fn get(&self, key: &K) -> Option<&V> {
        self.0.get(key)
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

// ── Queue ─────────────────────────────────────────────────────────────────

/// A double-ended queue.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Queue<T>(pub VecDeque<T>);

impl<T> Queue<T> {
    pub fn new() -> Self {
        Self(VecDeque::new())
    }
    pub fn push_back(&mut self, item: T) {
        self.0.push_back(item);
    }
    pub fn pop_front(&mut self) -> Option<T> {
        self.0.pop_front()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn peek_front(&self) -> Option<&T> {
        self.0.front()
    }
}

// ── Builders ──────────────────────────────────────────────────────────────

/// Builder for `List<T>`.
#[derive(Default)]
pub struct ListBuilder<T>(Vec<T>);

impl<T> ListBuilder<T> {
    pub fn new() -> Self {
        Self(Vec::new())
    }
    pub fn push(mut self, item: T) -> Self {
        self.0.push(item);
        self
    }
    pub fn build(self) -> List<T> {
        List(self.0)
    }
}

/// Builder for `Set<T>`.
pub struct SetBuilder<T: std::hash::Hash + Eq>(HashSet<T>);

impl<T: std::hash::Hash + Eq> SetBuilder<T> {
    pub fn new() -> Self {
        Self(HashSet::new())
    }
    pub fn insert(mut self, item: T) -> Self {
        self.0.insert(item);
        self
    }
    pub fn build(self) -> Set<T> {
        Set(self.0)
    }
}

impl<T: std::hash::Hash + Eq> Default for SetBuilder<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for `Map<K, V>`.
pub struct MapBuilder<K: std::hash::Hash + Eq, V>(HashMap<K, V>);

impl<K: std::hash::Hash + Eq, V> MapBuilder<K, V> {
    pub fn new() -> Self {
        Self(HashMap::new())
    }
    pub fn insert(mut self, key: K, value: V) -> Self {
        self.0.insert(key, value);
        self
    }
    pub fn build(self) -> Map<K, V> {
        Map(self.0)
    }
}

impl<K: std::hash::Hash + Eq, V> Default for MapBuilder<K, V> {
    fn default() -> Self {
        Self::new()
    }
}
