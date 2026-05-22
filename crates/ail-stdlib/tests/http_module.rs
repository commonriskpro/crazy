use ail_stdlib::http::{HeaderMap, HttpError, HttpMethod, HttpRequest, HttpResponse, StatusCode};

#[test]
fn header_map_insert_and_get() {
    let mut h = HeaderMap::new();
    h.insert("Content-Type", "application/json");
    assert_eq!(h.get("content-type"), Some("application/json"));
    assert_eq!(h.get("Content-Type"), Some("application/json"));
}

#[test]
fn header_map_case_insensitive() {
    let mut h = HeaderMap::new();
    h.insert("ACCEPT", "text/html");
    assert_eq!(h.get("accept"), Some("text/html"));
}

#[test]
fn status_code_ok() {
    let s = StatusCode::ok();
    assert_eq!(s.0, 200);
    assert!(s.is_success());
    assert!(!s.is_client_error());
    assert!(!s.is_server_error());
}

#[test]
fn status_code_not_found() {
    let s = StatusCode::not_found();
    assert!(s.is_client_error());
}

#[test]
fn status_code_server_error() {
    let s = StatusCode::internal_server_error();
    assert!(s.is_server_error());
}

#[test]
fn status_code_display() {
    assert_eq!(format!("{}", StatusCode::ok()), "200");
}

#[test]
fn http_method_display() {
    assert_eq!(format!("{}", HttpMethod::Get), "GET");
    assert_eq!(format!("{}", HttpMethod::Post), "POST");
    assert_eq!(format!("{}", HttpMethod::Custom("PURGE".into())), "PURGE");
}

#[test]
fn http_request_get() {
    let req = HttpRequest::get("https://example.com/api");
    assert_eq!(req.method, HttpMethod::Get);
    assert_eq!(req.url, "https://example.com/api");
    assert!(req.body.is_none());
}

#[test]
fn http_request_post_with_body() {
    let req = HttpRequest::post("https://example.com/api", b"data".to_vec());
    assert_eq!(req.method, HttpMethod::Post);
    assert_eq!(req.body, Some(b"data".to_vec()));
}

#[test]
fn http_request_with_header() {
    let req = HttpRequest::get("https://x.com").with_header("Authorization", "Bearer token");
    assert_eq!(req.headers.get("authorization"), Some("Bearer token"));
}

#[test]
fn http_response_body_as_text() {
    let resp = HttpResponse::new(StatusCode::ok(), b"OK body".to_vec());
    assert_eq!(resp.body_as_text().unwrap(), "OK body");
}

#[test]
fn http_response_invalid_utf8() {
    let resp = HttpResponse::new(StatusCode::ok(), vec![0xFF, 0xFE]);
    assert!(resp.body_as_text().is_err());
}

#[test]
fn http_error_display() {
    assert!(format!("{}", HttpError::PermissionDenied).contains("permission"));
    assert!(format!("{}", HttpError::Timeout).contains("timeout"));
    assert!(
        format!(
            "{}",
            HttpError::ServerError(StatusCode::internal_server_error())
        )
        .contains("server")
    );
}
