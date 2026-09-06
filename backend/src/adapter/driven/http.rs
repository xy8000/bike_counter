//! Shared ureq agent factory for the provider HTTP fetchers.
//!
//! Every adapter fetcher performs `ureq::get(...).call()` on ureq's default
//! agent, which has **no global timeout** — a server that accepts the connection
//! but stalls on the body can block an import worker thread forever (and with it
//! the finalization of the per-source import run, since cancellation is only
//! checked between provider calls). These helpers build an agent with an
//! end-to-end timeout and parse the optional `request_timeout_seconds` provider
//! var.

use std::time::Duration;

/// Default end-to-end timeout applied to every provider HTTP fetch (seconds).
pub const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 30;

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
