//! Bounded-concurrency fan-out for bulk operations. Uses the client's
//! shared limiter permits, so bulk work and other callers share one budget.

use std::future::Future;

use futures::{stream, StreamExt};

use crate::rate_limit::RateLimiter;

/// Run `f` over `items` with at most the limiter's permit count in flight.
/// Results keep the input order. `on_done` is called after each item (for
/// progress bars).
pub async fn run<T, R, F, Fut>(
    limiter: &RateLimiter,
    items: Vec<T>,
    f: F,
    on_done: impl Fn() + Send + Sync,
) -> Vec<R>
where
    F: Fn(T) -> Fut,
    Fut: Future<Output = R>,
{
    let f = &f;
    let on_done = &on_done;
    let mut indexed: Vec<(usize, R)> = stream::iter(items.into_iter().enumerate())
        .map(|(i, item)| async move {
            let _permit = limiter.permit().await;
            let r = f(item).await;
            on_done();
            (i, r)
        })
        .buffer_unordered(64)
        .collect()
        .await;
    indexed.sort_by_key(|(i, _)| *i);
    indexed.into_iter().map(|(_, r)| r).collect()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[tokio::test]
    async fn bounded_and_ordered() {
        let limiter = RateLimiter::new(3);
        let in_flight = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let done = AtomicUsize::new(0);
        let out = run(
            &limiter,
            (0..20).collect(),
            |i: usize| {
                let in_flight = &in_flight;
                let peak = &peak;
                async move {
                    let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    in_flight.fetch_sub(1, Ordering::SeqCst);
                    i * 2
                }
            },
            || {
                done.fetch_add(1, Ordering::SeqCst);
            },
        )
        .await;
        assert_eq!(out, (0..20).map(|i| i * 2).collect::<Vec<_>>());
        assert!(peak.load(Ordering::SeqCst) <= 3);
        assert_eq!(done.load(Ordering::SeqCst), 20);
    }
}
