//! Cached report for the **informational** health probes.
//!
//! [`HealthService`](crate::core::domain::health::service::HealthService) keeps
//! the critical (readiness) and informational (external provider / object
//! storage reachability) indicators apart. The informational probes are
//! synchronous TCP connects, so probing every configured source on every
//! `/health/providers` request would be needlessly expensive for a diagnostics
//! endpoint that a dashboard may poll.
//!
//! This cache evaluates the informational set at most once per TTL and reports
//! when the values were observed, so callers can tell a stale-but-cached report
//! apart from a fresh one. It never touches the critical set: readiness must
//! stay live.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};

use crate::core::domain::health::indicator::HealthComponent;
use crate::core::domain::health::service_port::HealthServicePort;

/// How long an informational report is reused before it is re-evaluated.
pub const DEFAULT_PROVIDER_HEALTH_TTL: Duration = Duration::from_secs(30);

/// A cached informational report plus the instant it was observed.
#[derive(Debug, Clone)]
pub struct ProviderHealthReport {
    /// One component per informational indicator, in registration order.
    pub components: Vec<HealthComponent>,
    /// When the indicators were evaluated.
    pub observed_at: DateTime<Utc>,
}

/// Caches the informational part of a [`HealthServicePort`].
pub struct ProviderHealthCache {
    service: Arc<dyn HealthServicePort>,
    ttl: Duration,
    cached: Mutex<Option<(Instant, ProviderHealthReport)>>,
}

impl ProviderHealthCache {
    /// Wraps `service` with a cache that reuses a report for `ttl`.
    pub fn new(service: Arc<dyn HealthServicePort>, ttl: Duration) -> Self {
        Self {
            service,
            ttl,
            cached: Mutex::new(None),
        }
    }

    /// Wraps `service` with the default 30 s TTL.
    pub fn with_default_ttl(service: Arc<dyn HealthServicePort>) -> Self {
        Self::new(service, DEFAULT_PROVIDER_HEALTH_TTL)
    }

    /// The current informational report, evaluating the indicators only when the
    /// cached value is missing or older than the TTL.
    ///
    /// The mutex is only held to read or store the cached value, never while the
    /// indicators run, so a slow probe cannot block a concurrent cached read.
    /// Two callers racing on an expired cache may both evaluate; that costs one
    /// extra set of probes and keeps the code free of a single-flight state
    /// machine.
    pub fn report(&self) -> ProviderHealthReport {
        {
            let guard = self
                .cached
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some((fetched_at, report)) = guard.as_ref()
                && fetched_at.elapsed() < self.ttl
            {
                return report.clone();
            }
        }

        let report = ProviderHealthReport {
            components: self.service.check_details(),
            observed_at: Utc::now(),
        };
        let mut guard = self
            .cached
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = Some((Instant::now(), report.clone()));
        report
    }
}

impl HealthServicePort for ProviderHealthCache {
    /// Delegates to the wrapped service: the critical set is never cached, so a
    /// readiness request always sees the live state.
    fn check(&self) -> Vec<HealthComponent> {
        self.service.check()
    }

    /// The cached informational report.
    fn check_details(&self) -> Vec<HealthComponent> {
        self.report().components
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::core::domain::health::indicator::HealthStatus;

    /// A fake port counting how often each indicator set is evaluated.
    struct CountingHealthService {
        critical_calls: Arc<AtomicUsize>,
        detail_calls: Arc<AtomicUsize>,
        detail_status: HealthStatus,
    }

    impl HealthServicePort for CountingHealthService {
        fn check(&self) -> Vec<HealthComponent> {
            self.critical_calls.fetch_add(1, Ordering::SeqCst);
            vec![HealthComponent {
                name: "postgres".to_string(),
                status: HealthStatus::Up,
            }]
        }

        fn check_details(&self) -> Vec<HealthComponent> {
            self.detail_calls.fetch_add(1, Ordering::SeqCst);
            vec![HealthComponent {
                name: "provider".to_string(),
                status: self.detail_status.clone(),
            }]
        }
    }

    fn service(
        status: HealthStatus,
    ) -> (
        Arc<CountingHealthService>,
        Arc<AtomicUsize>,
        Arc<AtomicUsize>,
    ) {
        let critical_calls = Arc::new(AtomicUsize::new(0));
        let detail_calls = Arc::new(AtomicUsize::new(0));
        let service = Arc::new(CountingHealthService {
            critical_calls: critical_calls.clone(),
            detail_calls: detail_calls.clone(),
            detail_status: status,
        });
        (service, critical_calls, detail_calls)
    }

    #[test]
    fn the_report_is_reused_within_the_ttl() {
        let (service, _, detail_calls) = service(HealthStatus::Up);
        let cache = ProviderHealthCache::new(service, Duration::from_secs(60));

        let first = cache.report();
        let second = cache.report();

        assert_eq!(detail_calls.load(Ordering::SeqCst), 1, "one evaluation");
        assert_eq!(first.components.len(), 1);
        assert_eq!(second.observed_at, first.observed_at);
    }

    #[test]
    fn the_report_is_reevaluated_after_the_ttl() {
        let (service, _, detail_calls) = service(HealthStatus::Down("offline".to_string()));
        let cache = ProviderHealthCache::new(service, Duration::ZERO);

        let first = cache.report();
        let second = cache.report();

        assert_eq!(detail_calls.load(Ordering::SeqCst), 2, "TTL elapsed");
        assert_eq!(
            first.components[0].status,
            HealthStatus::Down("offline".to_string()),
            "a down provider is reported, not turned into an error"
        );
        assert!(second.observed_at >= first.observed_at);
    }

    #[test]
    fn readiness_is_never_cached_but_details_are() {
        let (service, critical_calls, detail_calls) = service(HealthStatus::Up);
        let cache = ProviderHealthCache::with_default_ttl(service);

        assert_eq!(cache.check().len(), 1);
        assert_eq!(cache.check().len(), 1);
        assert_eq!(cache.check_details().len(), 1);
        assert_eq!(cache.check_details().len(), 1);

        assert_eq!(
            critical_calls.load(Ordering::SeqCst),
            2,
            "readiness is live"
        );
        assert_eq!(detail_calls.load(Ordering::SeqCst), 1, "details are cached");
    }
}
