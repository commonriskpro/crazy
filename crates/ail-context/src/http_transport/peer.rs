use super::HttpTransportError;

/// Returns `Ok(())` when `loopback_only` is `false` or the peer IP is a
/// loopback address; otherwise returns `Err(HttpTransportError::NonLoopback(addr))`.
///
/// Uses [`IpAddr::to_canonical`] before [`IpAddr::is_loopback`] so that
/// IPv4-mapped IPv6 loopback addresses (`::ffff:127.0.0.1`) are accepted on
/// dual-stack sockets alongside `127.0.0.1` and `::1`.
///
/// Extracted from [`HttpTransport::serve_one`] to allow exhaustive unit
/// testing of the IP classification logic without spinning up real TCP
/// connections.
pub(super) fn check_peer_addr(
    addr: std::net::SocketAddr,
    loopback_only: bool,
) -> Result<(), HttpTransportError> {
    if loopback_only && !addr.ip().to_canonical().is_loopback() {
        Err(HttpTransportError::NonLoopback(addr))
    } else {
        Ok(())
    }
}
