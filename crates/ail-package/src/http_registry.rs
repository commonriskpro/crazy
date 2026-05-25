// ── ail-package::http_registry ────────────────────────────────────────────
//
// Minimal HTTP/1.1 registry client and server.
//
// # Design
//
// This module provides an HTTP transport layer over the `RegistryClient` trait
// defined in `remote_registry`.  The server wraps `InMemoryRegistryClient`;
// the client implements `RegistryClient` over HTTP with JSON bodies.
//
// ## HTTP API
//
//   POST /api/v1/publish  — body: JSON `PublishRequest`  → JSON `PublishResponse`
//   POST /api/v1/fetch    — body: JSON `FetchRequest`    → JSON `FetchResponse`
//   POST /api/v1/search   — body: JSON `SearchRequest`   → JSON `SearchResponse`
//   POST /api/v1/verify   — body: JSON `VerifyRequest`   → JSON `VerifyResponse`
//
// ## Transport
//
// Uses `std::net::TcpListener` / `TcpStream` with hand-rolled HTTP/1.1 framing.
// The server is intentionally single-threaded (one request at a time) to avoid
// `RefCell` / `Send` complications from `InMemoryRegistryClient`.  Suitable for
// integration testing and ecosystem-path validation; not hardened for production.
//
// ## Ed25519 verification
//
// The server calls `signed.verify()` before accepting a publish, so every
// publish path exercises real Ed25519 signature verification.  Tests supply
// freshly generated `PackageKeypair` fixtures.
//
// # Scope (docs/decision-log.md)
//
// Sigstore-style / keyless signing and registry federation are out of scope
// for this module.  This implements the "simple HTTP registry path" first.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

// ── Safety limits ─────────────────────────────────────────────────────────

/// Maximum header block size accepted from a client (bytes).
const MAX_HEADER_SIZE: usize = 16_384;

/// Maximum request body size accepted from a client, and maximum response body
/// size the client will allocate.  Prevents a crafted `Content-Length` value
/// from triggering unbounded heap allocation; legitimate test payloads are
/// always well under this limit.
const MAX_BODY_SIZE: usize = 1_048_576; // 1 MiB

/// Read/write timeout applied to every accepted TCP connection (server-side)
/// and to every outgoing client connection.  Prevents a slow or stalled peer
/// from blocking the server's single accept thread indefinitely.
const IO_TIMEOUT: Duration = Duration::from_secs(5);

use crate::remote_registry::{
    FetchRequest, FetchResponse, InMemoryRegistryClient, PublishRequest, PublishResponse,
    RegistryClient, SearchRequest, SearchResponse, VerifyRequest, VerifyResponse,
};

// ── Internal HTTP framing helpers ─────────────────────────────────────────

/// A minimal parsed HTTP/1.1 request (method, path, body only).
struct RawRequest {
    method: String,
    path: String,
    body: Vec<u8>,
}

/// Read one HTTP/1.1 request from `stream`.
///
/// Reads byte-by-byte until `\r\n\r\n` (end of headers), then reads the
/// body according to `Content-Length`.  Returns `None` on any I/O or parse
/// error, or if the header exceeds 16 KiB.
fn read_request(stream: &mut TcpStream) -> Option<RawRequest> {
    // Read headers byte-by-byte until we see the CRLF CRLF terminator.
    let mut header_buf: Vec<u8> = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        stream.read_exact(&mut byte).ok()?;
        header_buf.push(byte[0]);
        if header_buf.ends_with(b"\r\n\r\n") {
            break;
        }
        if header_buf.len() > MAX_HEADER_SIZE {
            return None; // guard against oversized headers
        }
    }

    let header_text = std::str::from_utf8(&header_buf).ok()?;
    let mut lines = header_text.lines();

    // Parse the request line: METHOD path HTTP/1.x
    let request_line = lines.next()?;
    let mut parts = request_line.splitn(3, ' ');
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();

    // Scan headers for Content-Length.
    let mut content_length: usize = 0;
    for line in lines {
        let lower = line.to_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }

    // Reject excessively large bodies before allocating — prevents a crafted
    // Content-Length from becoming an OOM gadget.
    if content_length > MAX_BODY_SIZE {
        return None;
    }

    // Read the request body.
    let mut body = vec![0u8; content_length];
    stream.read_exact(&mut body).ok()?;

    Some(RawRequest { method, path, body })
}

/// Write a minimal HTTP/1.1 response with a JSON `body`.
fn write_response(stream: &mut TcpStream, status: u16, body: &[u8]) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Unknown",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {len}\r\n\
         Connection: close\r\n\
         \r\n",
        len = body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
}

// ── HttpRegistryServer ────────────────────────────────────────────────────

/// A minimal HTTP/1.1 server that exposes a package registry over TCP.
///
/// Backed by `InMemoryRegistryClient`.  The server handles one connection at
/// a time (no per-connection threads) so no `Send` bound is required on the
/// client state.
///
/// # Usage
///
/// ```rust,no_run
/// use ail_package::http_registry::HttpRegistryServer;
///
/// let addr = HttpRegistryServer::bind("127.0.0.1:0")
///     .expect("bind")
///     .spawn();
/// println!("listening on {addr}");
/// ```
pub struct HttpRegistryServer {
    listener: TcpListener,
}

impl HttpRegistryServer {
    /// Bind the server to `addr` (e.g. `"127.0.0.1:0"` for any free port).
    pub fn bind(addr: &str) -> std::io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        Ok(HttpRegistryServer { listener })
    }

    /// Return the local socket address the server is bound to.
    pub fn local_addr(&self) -> std::net::SocketAddr {
        self.listener
            .local_addr()
            .expect("bound listener has local addr")
    }

    /// Spawn the server accept loop on a background thread.
    ///
    /// Returns the `SocketAddr` the server is listening on.  The server runs
    /// until the spawned thread exits (which happens when the underlying
    /// `TcpListener` is dropped or the process exits).
    ///
    /// Each HTTP connection is handled synchronously before the next is
    /// accepted, keeping the registry state single-threaded.
    pub fn spawn(self) -> std::net::SocketAddr {
        let addr = self.local_addr();
        let listener = self.listener;
        std::thread::spawn(move || {
            let client = InMemoryRegistryClient::new();
            for stream in listener.incoming() {
                match stream {
                    Ok(mut conn) => {
                        let _ = conn.set_read_timeout(Some(IO_TIMEOUT));
                        let _ = conn.set_write_timeout(Some(IO_TIMEOUT));
                        handle_connection(&mut conn, &client);
                    }
                    Err(_) => break,
                }
            }
        });
        addr
    }
}

/// Handle one HTTP connection against the in-memory registry.
fn handle_connection(stream: &mut TcpStream, client: &InMemoryRegistryClient) {
    let Some(req) = read_request(stream) else {
        write_response(stream, 400, b"{\"error\":\"bad request\"}");
        return;
    };

    if req.method != "POST" {
        write_response(stream, 400, b"{\"error\":\"method must be POST\"}");
        return;
    }

    match req.path.as_str() {
        "/api/v1/publish" => {
            let Ok(pub_req) = serde_json::from_slice::<PublishRequest>(&req.body) else {
                write_response(stream, 400, b"{\"error\":\"invalid publish request\"}");
                return;
            };
            match client.publish(pub_req) {
                Ok(resp) => {
                    let body = serde_json::to_vec(&resp).unwrap_or_default();
                    write_response(stream, 200, &body);
                }
                Err(e) => {
                    let msg = serde_json::to_vec(&serde_json::json!({"error": e.to_string()}))
                        .unwrap_or_else(|_| b"{\"error\":\"internal error\"}".to_vec());
                    write_response(stream, 500, &msg);
                }
            }
        }
        "/api/v1/fetch" => {
            let Ok(fetch_req) = serde_json::from_slice::<FetchRequest>(&req.body) else {
                write_response(stream, 400, b"{\"error\":\"invalid fetch request\"}");
                return;
            };
            match client.fetch(fetch_req) {
                Ok(resp) => {
                    let body = serde_json::to_vec(&resp).unwrap_or_default();
                    write_response(stream, 200, &body);
                }
                Err(e) => {
                    let msg = serde_json::to_vec(&serde_json::json!({"error": e.to_string()}))
                        .unwrap_or_else(|_| b"{\"error\":\"internal error\"}".to_vec());
                    write_response(stream, 500, &msg);
                }
            }
        }
        "/api/v1/search" => {
            let Ok(search_req) = serde_json::from_slice::<SearchRequest>(&req.body) else {
                write_response(stream, 400, b"{\"error\":\"invalid search request\"}");
                return;
            };
            match client.search(search_req) {
                Ok(resp) => {
                    let body = serde_json::to_vec(&resp).unwrap_or_default();
                    write_response(stream, 200, &body);
                }
                Err(e) => {
                    let msg = serde_json::to_vec(&serde_json::json!({"error": e.to_string()}))
                        .unwrap_or_else(|_| b"{\"error\":\"internal error\"}".to_vec());
                    write_response(stream, 500, &msg);
                }
            }
        }
        "/api/v1/verify" => {
            let Ok(verify_req) = serde_json::from_slice::<VerifyRequest>(&req.body) else {
                write_response(stream, 400, b"{\"error\":\"invalid verify request\"}");
                return;
            };
            match client.verify(verify_req) {
                Ok(resp) => {
                    let body = serde_json::to_vec(&resp).unwrap_or_default();
                    write_response(stream, 200, &body);
                }
                Err(e) => {
                    let msg = serde_json::to_vec(&serde_json::json!({"error": e.to_string()}))
                        .unwrap_or_else(|_| b"{\"error\":\"internal error\"}".to_vec());
                    write_response(stream, 500, &msg);
                }
            }
        }
        _ => {
            write_response(stream, 404, b"{\"error\":\"not found\"}");
        }
    }
}

// ── HttpClientError ───────────────────────────────────────────────────────

/// Error type returned by `HttpRegistryClient` operations.
#[derive(Debug)]
pub enum HttpClientError {
    /// TCP-level I/O error.
    Transport(String),
    /// HTTP framing or protocol error.
    Protocol(String),
    /// JSON serialization / deserialization error.
    Json(String),
    /// Server returned a non-2xx HTTP status code.
    Server { status: u16, body: String },
}

impl std::fmt::Display for HttpClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpClientError::Transport(m) => write!(f, "transport: {m}"),
            HttpClientError::Protocol(m) => write!(f, "protocol: {m}"),
            HttpClientError::Json(m) => write!(f, "json: {m}"),
            HttpClientError::Server { status, body } => {
                write!(f, "server error {status}: {body}")
            }
        }
    }
}

impl std::error::Error for HttpClientError {}

// ── HttpRegistryClient ────────────────────────────────────────────────────

/// An HTTP/1.1 `RegistryClient` that POSTs JSON requests to a registry server.
///
/// `addr` is the `host:port` of the target server (no `http://` prefix),
/// e.g. `"127.0.0.1:8080"`.
///
/// # Verification
///
/// The server always verifies Ed25519 signatures on publish.  The client does
/// not need to re-verify on its side; it receives an explicit `accepted: false`
/// response if the server rejects a tampered package.
pub struct HttpRegistryClient {
    /// Registry server address (`host:port`).
    pub addr: String,
}

impl HttpRegistryClient {
    /// Create a client targeting `addr` (e.g. `"127.0.0.1:8080"`).
    pub fn new(addr: impl Into<String>) -> Self {
        HttpRegistryClient { addr: addr.into() }
    }

    /// Open a new TCP connection, POST `body` to `path`, return the response body.
    fn post_json(&self, path: &str, body: &[u8]) -> Result<Vec<u8>, HttpClientError> {
        let mut stream = TcpStream::connect(&self.addr)
            .map_err(|e| HttpClientError::Transport(e.to_string()))?;
        let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
        let _ = stream.set_write_timeout(Some(IO_TIMEOUT));

        // Write request.
        let header = format!(
            "POST {path} HTTP/1.1\r\n\
             Host: {addr}\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {len}\r\n\
             Connection: close\r\n\
             \r\n",
            addr = self.addr,
            len = body.len()
        );
        stream
            .write_all(header.as_bytes())
            .map_err(|e| HttpClientError::Transport(e.to_string()))?;
        stream
            .write_all(body)
            .map_err(|e| HttpClientError::Transport(e.to_string()))?;

        // Read response headers.
        let mut resp_buf: Vec<u8> = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            stream
                .read_exact(&mut byte)
                .map_err(|e| HttpClientError::Transport(e.to_string()))?;
            resp_buf.push(byte[0]);
            if resp_buf.ends_with(b"\r\n\r\n") {
                break;
            }
            if resp_buf.len() > 16_384 {
                return Err(HttpClientError::Protocol(
                    "response headers too large".into(),
                ));
            }
        }

        let header_text = std::str::from_utf8(&resp_buf)
            .map_err(|_| HttpClientError::Protocol("non-UTF-8 response headers".into()))?;

        // Parse HTTP status code from the response line: "HTTP/1.1 NNN Reason".
        let resp_status: u16 = header_text
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        // Extract Content-Length.
        let mut content_length: usize = 0;
        for line in header_text.lines() {
            let lower = line.to_lowercase();
            if let Some(value) = lower.strip_prefix("content-length:") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }

        // Reject excessively large response bodies before allocating.
        if content_length > MAX_BODY_SIZE {
            return Err(HttpClientError::Protocol("response body too large".into()));
        }

        // Read response body.
        let mut body_buf = vec![0u8; content_length];
        stream
            .read_exact(&mut body_buf)
            .map_err(|e| HttpClientError::Transport(e.to_string()))?;

        // Non-2xx → surface as a distinct error, not a JSON decode failure.
        if !(200..300).contains(&resp_status) {
            let body = String::from_utf8_lossy(&body_buf).into_owned();
            return Err(HttpClientError::Server {
                status: resp_status,
                body,
            });
        }

        Ok(body_buf)
    }
}

impl RegistryClient for HttpRegistryClient {
    type Error = HttpClientError;

    fn publish(&self, request: PublishRequest) -> Result<PublishResponse, Self::Error> {
        let body =
            serde_json::to_vec(&request).map_err(|e| HttpClientError::Json(e.to_string()))?;
        let resp = self.post_json("/api/v1/publish", &body)?;
        serde_json::from_slice(&resp).map_err(|e| HttpClientError::Json(e.to_string()))
    }

    fn fetch(&self, request: FetchRequest) -> Result<FetchResponse, Self::Error> {
        let body =
            serde_json::to_vec(&request).map_err(|e| HttpClientError::Json(e.to_string()))?;
        let resp = self.post_json("/api/v1/fetch", &body)?;
        serde_json::from_slice(&resp).map_err(|e| HttpClientError::Json(e.to_string()))
    }

    fn search(&self, request: SearchRequest) -> Result<SearchResponse, Self::Error> {
        let body =
            serde_json::to_vec(&request).map_err(|e| HttpClientError::Json(e.to_string()))?;
        let resp = self.post_json("/api/v1/search", &body)?;
        serde_json::from_slice(&resp).map_err(|e| HttpClientError::Json(e.to_string()))
    }

    fn verify(&self, request: VerifyRequest) -> Result<VerifyResponse, Self::Error> {
        let body =
            serde_json::to_vec(&request).map_err(|e| HttpClientError::Json(e.to_string()))?;
        let resp = self.post_json("/api/v1/verify", &body)?;
        serde_json::from_slice(&resp).map_err(|e| HttpClientError::Json(e.to_string()))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{PackageDef, PackageManifest};
    use crate::remote_registry::{PublishRequest, VerifyOutcome, VerifyRequest};
    use crate::signing::PackageKeypair;
    use crate::trust::TrustLevel;
    use rand::rngs::OsRng;
    use std::time::Duration;

    // ── Fixtures ──────────────────────────────────────────────────────────

    fn make_manifest(name: &str, version: &str) -> PackageManifest {
        PackageManifest::from_def(PackageDef {
            name: name.to_string(),
            version: version.to_string(),
            trust_level: TrustLevel::Verified,
            required_capabilities: vec![],
            exported_capabilities: vec![],
            assumptions: vec![],
            unsafe_surface: vec![],
            artifact_hashes: vec![],
            build_env_hash: None,
            handlers: vec![],
            contracts: vec![],
            exports: vec![],
            imports: vec![],
            boundaries: vec![],
            license: None,
            provenance: None,
            verification_report: None,
            graph_schema: None,
            core_ir_schema: None,
            reproducible_evidence: None,
        })
    }

    /// Generate a fresh Ed25519 keypair for test fixtures.
    fn gen_keypair() -> PackageKeypair {
        let secret = ed25519_dalek::SigningKey::generate(&mut OsRng);
        PackageKeypair::from_bytes(&secret.to_bytes())
    }

    /// Start a server on an OS-assigned port; return the addr string.
    fn start_server() -> String {
        let addr = HttpRegistryServer::bind("127.0.0.1:0")
            .expect("bind server")
            .spawn();
        addr.to_string()
    }

    // ── http_publish_and_fetch_roundtrip ──────────────────────────────────
    // Spec: HTTP publish + fetch returns the original signed package.
    //
    // Ed25519 fixture: a freshly generated keypair signs the manifest before
    // publish.  The server verifies the signature; the client retrieves the
    // identical signed package on fetch.
    #[test]
    fn http_publish_and_fetch_roundtrip() {
        let addr = start_server();
        let client = HttpRegistryClient::new(&addr);

        let kp = gen_keypair();
        let manifest = make_manifest("http.fetch.pkg", "1.0.0");
        let signed = kp.sign_manifest(manifest).expect("sign");

        let pub_resp = client
            .publish(PublishRequest {
                signed_package: signed.clone(),
            })
            .expect("publish transport");
        assert!(pub_resp.accepted, "publish must be accepted");
        assert!(pub_resp.error.is_none());

        let fetch_resp = client
            .fetch(crate::remote_registry::FetchRequest {
                name: "http.fetch.pkg".to_string(),
                version: "1.0.0".to_string(),
            })
            .expect("fetch transport");

        assert_eq!(
            fetch_resp.signed_package,
            Some(signed),
            "fetched package must equal published package"
        );
        assert!(!fetch_resp.yanked);
        assert!(fetch_resp.error.is_none());
    }

    // ── http_verify_ok ────────────────────────────────────────────────────
    // Spec: HTTP publish + verify returns Ok for matching hash.
    #[test]
    fn http_verify_ok() {
        let addr = start_server();
        let client = HttpRegistryClient::new(&addr);

        let kp = gen_keypair();
        let manifest = make_manifest("http.verify.ok.pkg", "2.0.0");
        let expected_hash = manifest.blake3_hex().expect("hash");
        let signed = kp.sign_manifest(manifest).expect("sign");

        client
            .publish(PublishRequest {
                signed_package: signed,
            })
            .expect("publish transport");

        let verify_resp = client
            .verify(VerifyRequest {
                name: "http.verify.ok.pkg".to_string(),
                version: "2.0.0".to_string(),
                expected_hash,
            })
            .expect("verify transport");

        assert_eq!(
            verify_resp.outcome,
            VerifyOutcome::Ok,
            "verify must return Ok for matching hash"
        );
    }

    // ── http_verify_hash_mismatch ─────────────────────────────────────────
    // Spec: HTTP verify returns HashMismatch when the client supplies a wrong hash.
    #[test]
    fn http_verify_hash_mismatch() {
        let addr = start_server();
        let client = HttpRegistryClient::new(&addr);

        let kp = gen_keypair();
        let manifest = make_manifest("http.hash.mismatch.pkg", "1.0.0");
        let signed = kp.sign_manifest(manifest).expect("sign");

        client
            .publish(PublishRequest {
                signed_package: signed,
            })
            .expect("publish transport");

        let verify_resp = client
            .verify(VerifyRequest {
                name: "http.hash.mismatch.pkg".to_string(),
                version: "1.0.0".to_string(),
                expected_hash: "a".repeat(64), // wrong hash
            })
            .expect("verify transport");

        assert!(
            matches!(verify_resp.outcome, VerifyOutcome::HashMismatch { .. }),
            "wrong hash must return HashMismatch"
        );
    }

    // ── http_verify_not_found ─────────────────────────────────────────────
    // Spec: HTTP verify returns NotFound for a package that was never published.
    #[test]
    fn http_verify_not_found() {
        let addr = start_server();
        let client = HttpRegistryClient::new(&addr);

        let verify_resp = client
            .verify(VerifyRequest {
                name: "nonexistent.pkg".to_string(),
                version: "1.0.0".to_string(),
                expected_hash: "b".repeat(64),
            })
            .expect("verify transport");

        assert_eq!(verify_resp.outcome, VerifyOutcome::NotFound);
    }

    // ── http_search ───────────────────────────────────────────────────────
    // Spec: HTTP search returns packages matching the query, deduped by latest version.
    #[test]
    fn http_search() {
        let addr = start_server();
        let client = HttpRegistryClient::new(&addr);

        let kp = gen_keypair();
        // Publish two versions; search must dedupe to one row (latest).
        for version in ["1.0.0", "2.0.0"] {
            let manifest = make_manifest("http.search.pkg", version);
            let signed = kp.sign_manifest(manifest).expect("sign");
            client
                .publish(PublishRequest {
                    signed_package: signed,
                })
                .expect("publish transport");
        }
        // Publish an unrelated package that should NOT match.
        let other = make_manifest("unrelated.pkg", "1.0.0");
        let other_signed = kp.sign_manifest(other).expect("sign");
        client
            .publish(PublishRequest {
                signed_package: other_signed,
            })
            .expect("publish transport");

        let search_resp = client
            .search(crate::remote_registry::SearchRequest {
                query: "http.search".to_string(),
                limit: None,
            })
            .expect("search transport");

        assert_eq!(
            search_resp.results.len(),
            1,
            "only one package name should match"
        );
        assert_eq!(search_resp.results[0].name, "http.search.pkg");
        assert_eq!(
            search_resp.results[0].latest_version, "2.0.0",
            "search must surface the latest version"
        );
    }

    // ── http_publish_tampered_rejected (Ed25519 fixture) ──────────────────
    // Spec: HTTP server rejects a tampered package via Ed25519 verification.
    //
    // Ed25519 fixture: the manifest is modified after signing (version bump),
    // invalidating the signature.  The server must refuse the publish.
    #[test]
    fn http_publish_tampered_rejected() {
        let addr = start_server();
        let client = HttpRegistryClient::new(&addr);

        let kp = gen_keypair();
        let manifest = make_manifest("tampered.pkg", "1.0.0");
        let mut signed = kp.sign_manifest(manifest).expect("sign");
        // Tamper: bump version after signing — the signature no longer covers "9.9.9".
        signed.manifest.version = "9.9.9".to_string();

        let pub_resp = client
            .publish(PublishRequest {
                signed_package: signed,
            })
            .expect("transport succeeds (server returns 200 with rejected body)");

        assert!(
            !pub_resp.accepted,
            "tampered package must be rejected (Ed25519 mismatch)"
        );
        assert!(
            pub_resp.error.is_some(),
            "rejection must include an error message"
        );
    }

    // ── http_fetch_not_found ──────────────────────────────────────────────
    // Spec: HTTP fetch returns an error message for an absent package.
    #[test]
    fn http_fetch_not_found() {
        let addr = start_server();
        let client = HttpRegistryClient::new(&addr);

        let resp = client
            .fetch(crate::remote_registry::FetchRequest {
                name: "absent.pkg".to_string(),
                version: "1.0.0".to_string(),
            })
            .expect("transport succeeds");

        assert!(resp.signed_package.is_none());
        assert!(
            resp.error.is_some(),
            "not-found fetch must include an error"
        );
    }

    // ── post_json_404_returns_server_error ───────────────────────────────
    // Spec: requesting an unknown path surfaces HttpClientError::Server { status: 404 },
    // not HttpClientError::Json.
    #[test]
    fn post_json_404_returns_server_error() {
        let addr = start_server();
        let client = HttpRegistryClient::new(&addr);
        let result = client.post_json("/nonexistent", b"{}");
        match result {
            Err(HttpClientError::Server { status, .. }) => {
                assert_eq!(status, 404, "unknown path must yield a 404 server error");
            }
            other => panic!("expected Server error, got {other:?}"),
        }
    }

    // ── post_json_400_bad_body_returns_server_error ──────────────────────
    // Spec: sending invalid JSON to a valid endpoint yields
    // HttpClientError::Server { status: 400 }, not HttpClientError::Json.
    #[test]
    fn post_json_400_bad_body_returns_server_error() {
        let addr = start_server();
        let client = HttpRegistryClient::new(&addr);
        let result = client.post_json("/api/v1/publish", b"this is not json");
        match result {
            Err(HttpClientError::Server { status, .. }) => {
                assert_eq!(status, 400, "malformed body must yield a 400 server error");
            }
            other => panic!("expected Server error, got {other:?}"),
        }
    }

    // ── post_json_500_returns_server_error ───────────────────────────────
    // Spec: a server returning 500 yields HttpClientError::Server { status: 500 },
    // with the body preserved for diagnosis.
    #[test]
    fn post_json_500_returns_server_error() {
        // Spawn a minimal server that always replies 500.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let bad_addr = listener.local_addr().unwrap().to_string();
        std::thread::spawn(move || {
            for mut conn in listener.incoming().flatten() {
                // Drain the full HTTP request before responding. A single
                // partial read can leave unread request bytes in the socket;
                // on macOS, dropping such a socket may send RST and make the
                // client observe `Connection reset` instead of the 500 body.
                let _ = read_request(&mut conn);
                write_response(&mut conn, 500, b"{\"error\":\"forced\"}");
            }
        });

        let client = HttpRegistryClient::new(&bad_addr);
        let result = client.post_json("/api/v1/publish", b"{}");
        match result {
            Err(HttpClientError::Server { status, body }) => {
                assert_eq!(status, 500, "forced 500 must yield a 500 server error");
                assert!(
                    body.contains("forced"),
                    "error body must be forwarded to caller: {body}"
                );
            }
            other => panic!("expected Server error, got {other:?}"),
        }
    }

    // ── http_publish_sequence_numbers ─────────────────────────────────────
    // Spec: successive publish responses carry monotonically increasing sequence numbers.
    #[test]
    fn http_publish_sequence_numbers() {
        let addr = start_server();
        let client = HttpRegistryClient::new(&addr);

        let kp = gen_keypair();
        let r1 = client
            .publish(PublishRequest {
                signed_package: kp
                    .sign_manifest(make_manifest("seq.pkg", "1.0.0"))
                    .expect("sign"),
            })
            .expect("publish 1");
        let r2 = client
            .publish(PublishRequest {
                signed_package: kp
                    .sign_manifest(make_manifest("seq.pkg", "2.0.0"))
                    .expect("sign"),
            })
            .expect("publish 2");

        assert!(r1.accepted);
        assert!(r2.accepted);
        // Sequence numbers must exist and be strictly increasing.
        let s1 = r1.sequence.expect("sequence in first publish");
        let s2 = r2.sequence.expect("sequence in second publish");
        assert!(
            s2 > s1,
            "sequence must be monotonically increasing: {s1} < {s2}"
        );
    }

    // ── http_publish_sequence_monotonic_on_same_version_republish ────────
    // Spec: re-publishing the same name/version through the HTTP path must still
    // advance the sequence number — it must not repeat the previous value or go
    // backward.  This is the HTTP-transport leg of the sequence-monotonicity
    // regression (W1 from the Wave 15C review).
    //
    //   GIVEN an HTTP registry server
    //   WHEN pkg v1.0.0 is published twice (same name/version) via HTTP
    //   THEN the second publish response has a strictly higher sequence number
    #[test]
    fn http_publish_sequence_monotonic_on_same_version_republish() {
        let addr = start_server();
        let client = HttpRegistryClient::new(&addr);

        let kp = gen_keypair();
        let r1 = client
            .publish(PublishRequest {
                signed_package: kp
                    .sign_manifest(make_manifest("http.mono.pkg", "1.0.0"))
                    .expect("sign first"),
            })
            .expect("first publish");
        // Re-publish identical name/version through HTTP.
        let r2 = client
            .publish(PublishRequest {
                signed_package: kp
                    .sign_manifest(make_manifest("http.mono.pkg", "1.0.0"))
                    .expect("sign second"),
            })
            .expect("second publish");

        assert!(r1.accepted);
        assert!(r2.accepted);

        let s1 = r1.sequence.expect("sequence on first publish");
        let s2 = r2.sequence.expect("sequence on second publish");
        assert!(
            s2 > s1,
            "re-publishing same name/version via HTTP must still advance sequence: s1={s1} s2={s2}"
        );
    }

    // ── header_too_large_returns_bad_request ─────────────────────────────
    // Spec: a request whose header block exceeds MAX_HEADER_SIZE bytes is
    // rejected with 400 without reading the body.
    #[test]
    fn header_too_large_returns_bad_request() {
        let addr = start_server();
        let mut stream = TcpStream::connect(&addr).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("set read timeout");

        // Padding header long enough to push the block past MAX_HEADER_SIZE.
        let padding = "A".repeat(20_000);
        let req = format!("POST /api/v1/publish HTTP/1.1\r\nX-Pad: {padding}\r\n\r\n");
        let _ = stream.write_all(req.as_bytes());

        // Capture the server's response headers.
        let mut resp_buf: Vec<u8> = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            if stream.read_exact(&mut byte).is_err() {
                break;
            }
            resp_buf.push(byte[0]);
            if resp_buf.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        let resp = String::from_utf8_lossy(&resp_buf);
        assert!(
            resp.starts_with("HTTP/1.1 400"),
            "oversized header block must yield 400, got: {resp}"
        );
    }

    // ── missing_content_length_returns_bad_request ───────────────────────
    // Spec: a POST with no Content-Length header defaults to a zero-length
    // body; JSON parsing of an empty body returns 400.
    #[test]
    fn missing_content_length_returns_bad_request() {
        let addr = start_server();
        let mut stream = TcpStream::connect(&addr).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("set read timeout");

        // No Content-Length — body defaults to empty.
        let req = "POST /api/v1/publish HTTP/1.1\r\nHost: localhost\r\n\r\n";
        stream.write_all(req.as_bytes()).expect("write request");

        let mut resp_buf: Vec<u8> = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            if stream.read_exact(&mut byte).is_err() {
                break;
            }
            resp_buf.push(byte[0]);
            if resp_buf.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        let resp = String::from_utf8_lossy(&resp_buf);
        assert!(
            resp.starts_with("HTTP/1.1 400"),
            "missing Content-Length (empty body) must yield 400, got: {resp}"
        );
    }

    // ── oversized_body_rejected ───────────────────────────────────────────
    // Spec: Content-Length exceeding MAX_BODY_SIZE is rejected with 400
    // before any body bytes are read or allocated.
    #[test]
    fn oversized_body_rejected() {
        let addr = start_server();
        let mut stream = TcpStream::connect(&addr).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("set read timeout");

        // Claim 2 MiB (double MAX_BODY_SIZE) without sending any body bytes.
        let req = "POST /api/v1/publish HTTP/1.1\r\nContent-Length: 2097152\r\n\r\n";
        stream.write_all(req.as_bytes()).expect("write request");

        let mut resp_buf: Vec<u8> = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            if stream.read_exact(&mut byte).is_err() {
                break;
            }
            resp_buf.push(byte[0]);
            if resp_buf.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        let resp = String::from_utf8_lossy(&resp_buf);
        assert!(
            resp.starts_with("HTTP/1.1 400"),
            "oversized Content-Length must yield 400, got: {resp}"
        );
    }
}
