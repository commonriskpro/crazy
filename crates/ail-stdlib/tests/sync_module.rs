use ail_stdlib::sync::{AilAtomicBool, AilAtomicI64, AilMutex, AilRwLock};

#[test]
fn mutex_read_and_write() {
    let m = AilMutex::new(0i32);
    m.with(|v| *v = 42);
    let result = m.with(|v| *v);
    assert_eq!(result, 42);
}

#[test]
fn mutex_clone_shares_state() {
    let m = AilMutex::new(0i32);
    let m2 = m.clone();
    m.with(|v| *v = 10);
    assert_eq!(m2.with(|v| *v), 10);
}

#[test]
fn rwlock_read() {
    let rw = AilRwLock::new(99i32);
    let v = rw.read(|x| *x);
    assert_eq!(v, 99);
}

#[test]
fn rwlock_write() {
    let rw = AilRwLock::new(0i32);
    rw.write(|x| *x = 55);
    assert_eq!(rw.read(|x| *x), 55);
}

#[test]
fn rwlock_clone_shares_state() {
    let rw = AilRwLock::new(0i32);
    let rw2 = rw.clone();
    rw.write(|x| *x = 77);
    assert_eq!(rw2.read(|x| *x), 77);
}

#[test]
fn atomic_bool_load_store() {
    let b = AilAtomicBool::new(false);
    assert!(!b.load());
    b.store(true);
    assert!(b.load());
}

#[test]
fn atomic_bool_swap() {
    let b = AilAtomicBool::new(false);
    let prev = b.swap(true);
    assert!(!prev);
    assert!(b.load());
}

#[test]
fn atomic_bool_clone_shares_state() {
    let b = AilAtomicBool::new(false);
    let b2 = b.clone();
    b.store(true);
    assert!(b2.load());
}

#[test]
fn atomic_i64_load_store() {
    let a = AilAtomicI64::new(0);
    assert_eq!(a.load(), 0);
    a.store(42);
    assert_eq!(a.load(), 42);
}

#[test]
fn atomic_i64_fetch_add() {
    let a = AilAtomicI64::new(10);
    let prev = a.fetch_add(5);
    assert_eq!(prev, 10);
    assert_eq!(a.load(), 15);
}

#[test]
fn atomic_i64_fetch_sub() {
    let a = AilAtomicI64::new(20);
    let prev = a.fetch_sub(3);
    assert_eq!(prev, 20);
    assert_eq!(a.load(), 17);
}

#[test]
fn atomic_i64_compare_exchange_success() {
    let a = AilAtomicI64::new(5);
    let r = a.compare_exchange(5, 10);
    assert_eq!(r, Ok(5));
    assert_eq!(a.load(), 10);
}

#[test]
fn atomic_i64_compare_exchange_failure() {
    let a = AilAtomicI64::new(5);
    let r = a.compare_exchange(999, 10);
    assert!(r.is_err());
    assert_eq!(a.load(), 5);
}
