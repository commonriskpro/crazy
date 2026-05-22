use ail_stdlib::bytes::Bytes;

#[test]
fn bytes_new_empty() {
    let b = Bytes::new();
    assert!(b.is_empty());
    assert_eq!(b.len(), 0);
}

#[test]
fn bytes_from_vec() {
    let b = Bytes::from_vec(vec![1, 2, 3]);
    assert_eq!(b.len(), 3);
    assert_eq!(b.as_slice(), &[1, 2, 3]);
}

#[test]
fn bytes_concat() {
    let a = Bytes::from_slice(b"hello");
    let b = Bytes::from_slice(b" world");
    let c = a.concat(&b);
    assert_eq!(c.as_slice(), b"hello world");
}

#[test]
fn bytes_slice_in_bounds() {
    let b = Bytes::from_slice(b"abcdef");
    let s = b.slice(1, 4).unwrap();
    assert_eq!(s.as_slice(), b"bcd");
}

#[test]
fn bytes_slice_out_of_bounds() {
    let b = Bytes::from_slice(b"abc");
    assert!(b.slice(1, 10).is_none());
}

#[test]
fn bytes_to_text_valid_utf8() {
    let b = Bytes::from_slice(b"hello");
    assert_eq!(b.to_text(), Ok("hello".to_string()));
}

#[test]
fn bytes_to_text_invalid_utf8() {
    let b = Bytes::from_vec(vec![0xFF, 0xFE]);
    assert!(b.to_text().is_err());
}

#[test]
fn bytes_from_impl() {
    let b: Bytes = vec![1u8, 2, 3].into();
    assert_eq!(b.len(), 3);
}
