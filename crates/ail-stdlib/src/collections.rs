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

// ── Collection diagnostics ───────────────────────────────────────────────

/// Stable collection domains for redacted diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CollectionKind {
    List,
    Set,
    Map,
    Queue,
}

impl CollectionKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Set => "set",
            Self::Map => "map",
            Self::Queue => "queue",
        }
    }
}

/// Stable collection operation names for diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CollectionOperation {
    Get,
    Insert,
    PopFront,
}

impl CollectionOperation {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Get => "get",
            Self::Insert => "insert",
            Self::PopFront => "pop_front",
        }
    }
}

/// Stable collection issue kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CollectionIssueKind {
    IndexOutOfBounds,
    DuplicateItem,
    DuplicateKey,
    EmptyQueue,
}

impl CollectionIssueKind {
    pub const fn code(self) -> &'static str {
        match self {
            Self::IndexOutOfBounds => "std.collections.index.out_of_bounds",
            Self::DuplicateItem => "std.collections.set.duplicate_item",
            Self::DuplicateKey => "std.collections.map.duplicate_key",
            Self::EmptyQueue => "std.collections.queue.empty",
        }
    }

    pub const fn category(self) -> &'static str {
        match self {
            Self::IndexOutOfBounds => "bounds",
            Self::DuplicateItem | Self::DuplicateKey => "uniqueness",
            Self::EmptyQueue => "state",
        }
    }
}

/// Machine-readable collection issue that exposes shape, not stored values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CollectionIssue {
    pub collection: CollectionKind,
    pub operation: CollectionOperation,
    pub kind: CollectionIssueKind,
    pub len: Option<usize>,
    pub index: Option<usize>,
}

impl CollectionIssue {
    pub const fn code(&self) -> &'static str {
        self.kind.code()
    }

    pub const fn category(&self) -> &'static str {
        self.kind.category()
    }

    pub const fn collection_label(&self) -> &'static str {
        self.collection.label()
    }

    pub const fn operation_label(&self) -> &'static str {
        self.operation.label()
    }

    pub fn diagnostic_key(&self) -> String {
        format!(
            "std.collections:{}:{}:{}",
            self.collection_label(),
            self.category(),
            self.code()
        )
    }
}

fn collection_issue(
    collection: CollectionKind,
    operation: CollectionOperation,
    kind: CollectionIssueKind,
    len: Option<usize>,
    index: Option<usize>,
) -> CollectionIssue {
    CollectionIssue {
        collection,
        operation,
        kind,
        len,
        index,
    }
}

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
    pub fn try_get(&self, index: usize) -> Result<&T, CollectionIssue> {
        self.0.get(index).ok_or_else(|| {
            collection_issue(
                CollectionKind::List,
                CollectionOperation::Get,
                CollectionIssueKind::IndexOutOfBounds,
                Some(self.len()),
                Some(index),
            )
        })
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
    pub fn try_insert_unique(&mut self, item: T) -> Result<(), CollectionIssue> {
        if self.0.insert(item) {
            Ok(())
        } else {
            Err(collection_issue(
                CollectionKind::Set,
                CollectionOperation::Insert,
                CollectionIssueKind::DuplicateItem,
                Some(self.len()),
                None,
            ))
        }
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
    pub fn try_insert_unique(&mut self, item: T) -> Result<(), CollectionIssue> {
        if self.0.insert(item) {
            Ok(())
        } else {
            Err(collection_issue(
                CollectionKind::Set,
                CollectionOperation::Insert,
                CollectionIssueKind::DuplicateItem,
                Some(self.len()),
                None,
            ))
        }
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
    pub fn try_insert_unique(&mut self, key: K, value: V) -> Result<(), CollectionIssue> {
        let len_before = self.len();
        match self.0.entry(key) {
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert(value);
                Ok(())
            }
            std::collections::hash_map::Entry::Occupied(_) => Err(collection_issue(
                CollectionKind::Map,
                CollectionOperation::Insert,
                CollectionIssueKind::DuplicateKey,
                Some(len_before),
                None,
            )),
        }
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
    pub fn try_insert_unique(&mut self, key: K, value: V) -> Result<(), CollectionIssue> {
        let len_before = self.len();
        match self.0.entry(key) {
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(value);
                Ok(())
            }
            std::collections::btree_map::Entry::Occupied(_) => Err(collection_issue(
                CollectionKind::Map,
                CollectionOperation::Insert,
                CollectionIssueKind::DuplicateKey,
                Some(len_before),
                None,
            )),
        }
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
    pub fn try_pop_front(&mut self) -> Result<T, CollectionIssue> {
        self.0.pop_front().ok_or_else(|| {
            collection_issue(
                CollectionKind::Queue,
                CollectionOperation::PopFront,
                CollectionIssueKind::EmptyQueue,
                Some(0),
                None,
            )
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_try_get_reports_redacted_bounds_issue() {
        let list = List::from_vec(vec!["secret-a", "secret-b"]);

        let issue = list.try_get(5).unwrap_err();

        assert_eq!(issue.code(), "std.collections.index.out_of_bounds");
        assert_eq!(issue.category(), "bounds");
        assert_eq!(issue.collection_label(), "list");
        assert_eq!(issue.operation_label(), "get");
        assert_eq!(issue.len, Some(2));
        assert_eq!(issue.index, Some(5));
        assert!(!issue.diagnostic_key().contains("secret"));
    }

    #[test]
    fn set_try_insert_unique_reports_duplicate_without_value() {
        let mut set = Set::new();
        assert_eq!(set.try_insert_unique("token-1"), Ok(()));

        let issue = set.try_insert_unique("token-1").unwrap_err();

        assert_eq!(issue.code(), "std.collections.set.duplicate_item");
        assert_eq!(issue.category(), "uniqueness");
        assert_eq!(issue.collection_label(), "set");
        assert_eq!(issue.len, Some(1));
        assert!(!issue.diagnostic_key().contains("token"));
    }

    #[test]
    fn map_try_insert_unique_reports_duplicate_without_key() {
        let mut map = Map::new();
        assert_eq!(map.try_insert_unique("api_key", 1), Ok(()));

        let issue = map.try_insert_unique("api_key", 2).unwrap_err();

        assert_eq!(issue.code(), "std.collections.map.duplicate_key");
        assert_eq!(issue.category(), "uniqueness");
        assert_eq!(issue.collection_label(), "map");
        assert_eq!(issue.operation_label(), "insert");
        assert_eq!(issue.len, Some(1));
        assert!(!issue.diagnostic_key().contains("api_key"));
    }

    #[test]
    fn queue_try_pop_front_reports_empty_state() {
        let mut queue: Queue<String> = Queue::new();

        let issue = queue.try_pop_front().unwrap_err();

        assert_eq!(issue.code(), "std.collections.queue.empty");
        assert_eq!(issue.category(), "state");
        assert_eq!(issue.collection_label(), "queue");
        assert_eq!(issue.operation_label(), "pop_front");
        assert_eq!(issue.len, Some(0));
        assert_eq!(
            issue.diagnostic_key(),
            "std.collections:queue:state:std.collections.queue.empty"
        );
    }
}
