use ail_stdlib::io::{InMemoryStream, IoError, Mode, Reader, Writer};

#[test]
fn in_memory_stream_write_and_read() {
    let mut stream = InMemoryStream::new();
    stream.write(b"hello").unwrap();
    stream.write(b" world").unwrap();

    let mut read_stream = InMemoryStream::from_bytes(stream.into_bytes());
    let mut buf = [0u8; 11];
    let n = read_stream.read(&mut buf).unwrap();
    assert_eq!(n, 11);
    assert_eq!(&buf, b"hello world");
}

#[test]
fn in_memory_stream_read_all() {
    let mut stream = InMemoryStream::from_bytes(b"test data".to_vec());
    let data = stream.read_all().unwrap();
    assert_eq!(data, b"test data");
}

#[test]
fn in_memory_stream_empty_read() {
    let mut stream = InMemoryStream::new();
    let data = stream.read_all().unwrap();
    assert!(data.is_empty());
}

#[test]
fn in_memory_stream_partial_reads() {
    let mut stream = InMemoryStream::from_bytes(vec![1, 2, 3, 4, 5]);
    let mut buf = [0u8; 2];
    let n1 = stream.read(&mut buf).unwrap();
    assert_eq!(n1, 2);
    assert_eq!(&buf, &[1, 2]);
    let n2 = stream.read(&mut buf).unwrap();
    assert_eq!(n2, 2);
    assert_eq!(&buf, &[3, 4]);
}

#[test]
fn in_memory_stream_flush_ok() {
    let mut stream = InMemoryStream::new();
    assert_eq!(stream.flush(), Ok(()));
}

#[test]
fn io_error_display() {
    assert_eq!(
        format!("{}", IoError::PermissionDenied),
        "permission denied"
    );
    assert_eq!(format!("{}", IoError::NotFound), "not found");
    assert_eq!(
        format!("{}", IoError::Other("oops".into())),
        "io error: oops"
    );
}

#[test]
fn mode_variants() {
    let _ = Mode::Read;
    let _ = Mode::Write;
    let _ = Mode::ReadWrite;
    let _ = Mode::Append;
}
