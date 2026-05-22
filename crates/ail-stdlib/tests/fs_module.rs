use ail_stdlib::fs::{FileError, FileMetadata, FsCapability, Path};

#[test]
fn path_new_and_as_str() {
    let p = Path::new("/tmp/test.txt");
    assert_eq!(p.as_str(), "/tmp/test.txt");
}

#[test]
fn path_join() {
    let p = Path::new("/var/data");
    let child = p.join("file.txt");
    assert_eq!(child.as_str(), "/var/data/file.txt");
}

#[test]
fn path_file_name() {
    let p = Path::new("/var/data/file.txt");
    assert_eq!(p.file_name(), Some("file.txt"));
}

#[test]
fn path_file_name_root() {
    let p = Path::new("/");
    assert_eq!(p.file_name(), None);
}

#[test]
fn path_parent() {
    let p = Path::new("/var/data/file.txt");
    assert_eq!(p.parent().unwrap().as_str(), "/var/data");
}

#[test]
fn path_display() {
    let p = Path::new("/foo/bar");
    assert_eq!(format!("{p}"), "/foo/bar");
}

#[test]
fn file_error_display() {
    let p = Path::new("/etc/secret");
    assert!(format!("{}", FileError::NotFound(p.clone())).contains("not found"));
    assert!(format!("{}", FileError::PermissionDenied(p.clone())).contains("permission denied"));
    assert!(format!("{}", FileError::AlreadyExists(p.clone())).contains("already exists"));
    assert!(format!("{}", FileError::IsDirectory(p.clone())).contains("directory"));
    assert!(format!("{}", FileError::Other("oops".into())).contains("oops"));
}

#[test]
fn fs_capability_variants() {
    let _ = FsCapability::Read;
    let _ = FsCapability::Write;
    let _ = FsCapability::Delete;
    let _ = FsCapability::List;
}

#[test]
fn file_metadata_fields() {
    let m = FileMetadata {
        path: Path::new("/tmp/x"),
        size_bytes: 1024,
        is_file: true,
        is_dir: false,
    };
    assert_eq!(m.size_bytes, 1024);
    assert!(m.is_file);
    assert!(!m.is_dir);
}
