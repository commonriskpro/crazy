use ail_stdlib::concurrent::{CancellationToken, Channel, TaskGroup, Timeout};

#[test]
fn cancellation_token_initial_state() {
    let token = CancellationToken::new();
    assert!(!token.is_cancelled());
}

#[test]
fn cancellation_token_cancel() {
    let token = CancellationToken::new();
    token.cancel();
    assert!(token.is_cancelled());
}

#[test]
fn cancellation_token_clone_shares_state() {
    let token = CancellationToken::new();
    let clone = token.clone();
    token.cancel();
    assert!(clone.is_cancelled());
}

#[test]
fn task_group_spawn_count() {
    let mut group = TaskGroup::new();
    group.spawn();
    group.spawn();
    assert_eq!(group.task_count, 2);
}

#[test]
fn task_group_cancel_all() {
    let group = TaskGroup::new();
    group.cancel_all();
    assert!(group.token.is_cancelled());
}

#[test]
fn channel_send_and_recv() {
    let ch: Channel<i32> = Channel::new(4);
    ch.send(1).unwrap();
    ch.send(2).unwrap();
    assert_eq!(ch.recv(), Some(1));
    assert_eq!(ch.recv(), Some(2));
    assert_eq!(ch.recv(), None);
}

#[test]
fn channel_full_returns_err() {
    let ch: Channel<i32> = Channel::new(2);
    ch.send(1).unwrap();
    ch.send(2).unwrap();
    assert!(ch.send(3).is_err());
}

#[test]
fn channel_len_and_empty() {
    let ch: Channel<i32> = Channel::new(10);
    assert!(ch.is_empty());
    ch.send(42).unwrap();
    assert_eq!(ch.len(), 1);
    assert!(!ch.is_empty());
}

#[test]
fn channel_clone_shares_queue() {
    let ch: Channel<i32> = Channel::new(10);
    let ch2 = ch.clone();
    ch.send(99).unwrap();
    assert_eq!(ch2.recv(), Some(99));
}

#[test]
fn timeout_constructors() {
    let t = Timeout::from_millis(250);
    assert_eq!(t.millis, 250);
    let t2 = Timeout::from_secs(3);
    assert_eq!(t2.millis, 3000);
}
