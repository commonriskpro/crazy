use ail_stdlib::net::{NetError, RetryPolicy, Timeout, Url};

#[test]
fn url_parse_basic() {
    let url = Url::parse("https://example.com/path").unwrap();
    assert_eq!(url.scheme, "https");
    assert_eq!(url.host, "example.com");
    assert_eq!(url.path, "/path");
    assert!(url.port.is_none());
}

#[test]
fn url_parse_with_port() {
    let url = Url::parse("http://localhost:8080/api").unwrap();
    assert_eq!(url.host, "localhost");
    assert_eq!(url.port, Some(8080));
    assert_eq!(url.path, "/api");
}

#[test]
fn url_parse_invalid_no_scheme() {
    assert!(Url::parse("not-a-url").is_err());
}

#[test]
fn url_to_string_basic() {
    let url = Url::new("https", "api.example.com", "/v1/data");
    assert_eq!(url.to_string(), "https://api.example.com/v1/data");
}

#[test]
fn net_error_display() {
    assert!(format!("{}", NetError::PermissionDenied).contains("permission"));
    assert!(format!("{}", NetError::Timeout).contains("timeout"));
    assert!(format!("{}", NetError::DnsFailure("host".into())).contains("dns"));
    assert!(format!("{}", NetError::ConnectionFailed("conn".into())).contains("connection"));
    assert!(format!("{}", NetError::InvalidUrl("x".into())).contains("url"));
}

#[test]
fn timeout_constructors() {
    let t = Timeout::from_millis(500);
    assert_eq!(t.millis, 500);
    let t2 = Timeout::from_secs(2);
    assert_eq!(t2.millis, 2000);
}

#[test]
fn retry_policy_no_retry() {
    let r = RetryPolicy::no_retry();
    assert_eq!(r.max_attempts, 1);
    assert_eq!(r.backoff_millis, 0);
}

#[test]
fn retry_policy_fixed() {
    let r = RetryPolicy::fixed(3, 100);
    assert_eq!(r.max_attempts, 3);
    assert_eq!(r.backoff_millis, 100);
}
