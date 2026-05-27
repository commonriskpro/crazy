use std::io::Write;

/// Write a minimal HTTP/1.1 response and flush the writer.
///
/// Always emits `Content-Type: application/json` and `Connection: close`.
/// For non-200 responses the body is plain text, but a uniform content-type
/// header simplifies client parsing.
pub(super) fn write_http_response<W: Write>(
    writer: &mut W,
    status: u16,
    reason: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let body_len = body.len();
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {body_len}\r\n\
         Connection: close\r\n\
         \r\n"
    );
    writer.write_all(header.as_bytes())?;
    writer.write_all(body)?;
    writer.flush()
}
