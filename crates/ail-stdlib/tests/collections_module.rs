use ail_stdlib::collections::{
    List, ListBuilder, Map, MapBuilder, OrderedMap, OrderedSet, Queue, Set, SetBuilder,
};

#[test]
fn list_push_and_get() {
    let mut l: List<i32> = List::new();
    l.push(1);
    l.push(2);
    l.push(3);
    assert_eq!(l.len(), 3);
    assert_eq!(l.get(1), Some(&2));
}

#[test]
fn list_is_empty() {
    let l: List<i32> = List::new();
    assert!(l.is_empty());
}

#[test]
fn list_concat() {
    let a = List::from_vec(vec![1, 2]);
    let b = List::from_vec(vec![3, 4]);
    let c = a.concat(&b);
    assert_eq!(c.as_slice(), &[1, 2, 3, 4]);
}

#[test]
fn list_builder() {
    let l = ListBuilder::new().push(10).push(20).build();
    assert_eq!(l.len(), 2);
    assert_eq!(l.get(0), Some(&10));
}

#[test]
fn set_no_duplicates() {
    let mut s: Set<i32> = Set::new();
    s.insert(1);
    s.insert(1);
    s.insert(2);
    assert_eq!(s.len(), 2);
    assert!(s.contains(&1));
    assert!(s.contains(&2));
}

#[test]
fn set_remove() {
    let mut s: Set<i32> = Set::new();
    s.insert(42);
    assert!(s.remove(&42));
    assert!(!s.contains(&42));
}

#[test]
fn set_builder() {
    let s = SetBuilder::new().insert(1).insert(2).insert(1).build();
    assert_eq!(s.len(), 2);
}

#[test]
fn map_unique_keys() {
    let mut m: Map<String, i32> = Map::new();
    m.insert("a".into(), 1);
    m.insert("a".into(), 2);
    assert_eq!(m.len(), 1);
    assert_eq!(m.get(&"a".to_string()), Some(&2));
}

#[test]
fn map_remove() {
    let mut m: Map<String, i32> = Map::new();
    m.insert("x".into(), 99);
    m.remove(&"x".to_string());
    assert!(m.is_empty());
}

#[test]
fn map_builder() {
    let m = MapBuilder::new().insert("k", 1).insert("v", 2).build();
    assert_eq!(m.len(), 2);
}

#[test]
fn ordered_set_sorted_iteration() {
    let mut s: OrderedSet<i32> = OrderedSet::new();
    s.insert(3);
    s.insert(1);
    s.insert(2);
    let v: Vec<&i32> = s.0.iter().collect();
    assert_eq!(v, vec![&1, &2, &3]);
}

#[test]
fn ordered_map_sorted_keys() {
    let mut m: OrderedMap<String, i32> = OrderedMap::new();
    m.insert("b".into(), 2);
    m.insert("a".into(), 1);
    let keys: Vec<&str> = m.0.keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["a", "b"]);
}

#[test]
fn queue_push_pop() {
    let mut q: Queue<i32> = Queue::new();
    q.push_back(1);
    q.push_back(2);
    assert_eq!(q.pop_front(), Some(1));
    assert_eq!(q.pop_front(), Some(2));
    assert!(q.is_empty());
}

#[test]
fn queue_peek() {
    let mut q: Queue<i32> = Queue::new();
    q.push_back(42);
    assert_eq!(q.peek_front(), Some(&42));
    assert!(!q.is_empty());
}
