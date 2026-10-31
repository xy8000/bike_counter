//! Shared ureq agent factory for the provider HTTP fetchers.
//!
//! Every adapter fetcher performs `ureq::get(...).call()` on ureq's default
//! agent, which has **no global timeout** — a server that accepts the connection
//! but stalls on the body can block an import worker thread forever (and with it
//! the finalization of the per-source import run, since cancellation is only
//! checked between provider calls). These helpers build an agent with an
//! end-to-end timeout and parse the optional `request_timeout_seconds` provider
//! var.

use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::core::domain::health::indicator::HealthStatus;

/// Default end-to-end timeout applied to every provider HTTP fetch (seconds).
pub const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 30;

/// Default timeout for the informational provider health probes (seconds).
///
/// Kept short: the probe only feeds `/health/providers`, which never gates
/// readiness, so waiting longer than a couple of seconds buys nothing.
pub const DEFAULT_TCP_PROBE_TIMEOUT_SECS: u64 = 2;

/// The default probe timeout as a [`Duration`].
pub fn default_tcp_probe_timeout() -> Duration {
    Duration::from_secs(DEFAULT_TCP_PROBE_TIMEOUT_SECS)
}

/// Maps a parsed provider endpoint to a health status through a bounded TCP
/// probe, so every adapter reports provider reachability identically.
///
/// `field` names the provider key that holds the URL, so an unparsable value
/// still yields a message pointing at the offending configuration key. A raw
/// `TcpStream::connect` has no timeout and used to block the (informational)
/// health check for minutes; the probe is bounded to
/// [`DEFAULT_TCP_PROBE_TIMEOUT_SECS`].
pub fn provider_endpoint_health(endpoint: Option<(String, u16)>, field: &str) -> HealthStatus {
    let Some((host, port)) = endpoint else {
        return HealthStatus::Down(format!("invalid {field} in provider config"));
    };
    match tcp_reachable(&host, port, default_tcp_probe_timeout()) {
        Ok(()) => HealthStatus::Up,
        Err(error) => HealthStatus::Down(error),
    }
}

/// Probes whether `host:port` accepts a TCP connection within `timeout`.
///
/// Resolves the host through [`ToSocketAddrs`] and tries every resolved address
/// with [`TcpStream::connect_timeout`]: unlike a raw `TcpStream::connect`, the
/// attempt can never block for the operating system's default connect timeout
/// (minutes), which is what used to make a readiness probe hang on an
/// unreachable provider. The last error is returned when no address connects.
pub fn tcp_reachable(host: &str, port: u16, timeout: Duration) -> Result<(), String> {
    let addresses: Vec<_> = (host, port)
        .to_socket_addrs()
        .map_err(|error| format!("failed to resolve '{host}': {error}"))?
        .collect();
    if addresses.is_empty() {
        return Err(format!("'{host}:{port}' resolved to no addresses"));
    }

    let mut last_error = None;
    for address in addresses {
        match TcpStream::connect_timeout(&address, timeout) {
            Ok(_) => return Ok(()),
            Err(error) => last_error = Some(format!("{address}: {error}")),
        }
    }
    Err(last_error.unwrap_or_else(|| format!("'{host}:{port}' is unreachable")))
}

/// Builds a ureq agent with an explicit end-to-end timeout covering DNS, connect
/// and reading the whole response body ([ureq `timeout_global`]).
///
/// [ureq `timeout_global`]: https://docs.rs/ureq/latest/ureq/struct.ConfigBuilder.html#method.timeout_global
pub fn timed_agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build()
        .into()
}

/// Parses the optional `request_timeout_seconds` provider var (whole seconds;
/// defaults to [`DEFAULT_REQUEST_TIMEOUT_SECS`]). Returns a human-readable
/// message so adapters can surface an invalid value as a configuration error.
pub fn parse_request_timeout(var: Option<&str>) -> Result<Duration, String> {
    let secs = match var {
        None => DEFAULT_REQUEST_TIMEOUT_SECS,
        Some(raw) => raw.parse::<u64>().map_err(|_| {
            format!("var 'request_timeout_seconds' is not a whole number of seconds: '{raw}'")
        })?,
    };
    Ok(Duration::from_secs(secs))
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    use super::{default_tcp_probe_timeout, provider_endpoint_health, tcp_reachable};
    use crate::core::domain::health::indicator::HealthStatus;

    #[test]
    fn a_listening_port_is_reachable() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
        let port = listener.local_addr().unwrap().port();

        assert!(tcp_reachable("127.0.0.1", port, default_tcp_probe_timeout()).is_ok());
    }

    #[test]
    fn a_closed_port_returns_within_the_timeout() {
        // Bind and drop, so the port is (almost certainly) closed afterwards.
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
            listener.local_addr().unwrap().port()
        };

        let timeout = Duration::from_millis(200);
        let started = Instant::now();
        let result = tcp_reachable("127.0.0.1", port, timeout);

        assert!(result.is_err(), "a closed port must not be reachable");
        // A refused connection returns immediately; the timeout only bounds the
        // worst case (a black-holed address).
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the probe must not block, took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn an_unresolvable_host_returns_an_error_instead_of_hanging() {
        let result = tcp_reachable("does-not-exist.invalid", 443, default_tcp_probe_timeout());
        assert!(result.is_err());
    }

    #[test]
    fn the_default_probe_timeout_is_two_seconds() {
        assert_eq!(default_tcp_probe_timeout(), Duration::from_secs(2));
    }

    #[test]
    fn an_unparsable_endpoint_names_the_offending_config_field() {
        let status = provider_endpoint_health(None, "base_url");
        assert_eq!(
            status,
            HealthStatus::Down("invalid base_url in provider config".to_string())
        );
    }

    #[test]
    fn a_reachable_endpoint_reports_up_and_a_closed_one_down() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
        let open_port = listener.local_addr().unwrap().port();
        assert_eq!(
            provider_endpoint_health(Some(("127.0.0.1".to_string(), open_port)), "url"),
            HealthStatus::Up
        );

        let closed_port = {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
            listener.local_addr().unwrap().port()
        };
        assert!(matches!(
            provider_endpoint_health(Some(("127.0.0.1".to_string(), closed_port)), "url"),
            HealthStatus::Down(_)
        ));
    }
}
