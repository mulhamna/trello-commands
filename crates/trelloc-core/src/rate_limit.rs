//! Client-side pacing for Trello's rolling-window limits. Trello sends no
//! reset header, so instead of reacting to 429s we never send more than
//! N requests in any rolling window:
//!
//! - token: 90 / 10 s for every request (Trello allows 100),
//! - members: 90 / 900 s for top-level `/members/{id}` (Trello allows 100),
//! - search: 10 / 10 s for `/search` (Trello's value is unpublished).
//!
//! Shared via `Arc` so bulk commands and concurrent MCP calls draw from the
//! same budget. Uses `tokio::time` so tests can run on a paused clock.

use std::{collections::VecDeque, sync::Mutex, time::Duration};

use reqwest::header::HeaderMap;
use tokio::{
    sync::{Semaphore, SemaphorePermit},
    time::Instant,
};

pub const DEFAULT_CONCURRENCY: usize = 4;
pub const TOKEN_LIMIT: usize = 90;
pub const TOKEN_WINDOW: Duration = Duration::from_secs(10);
pub const MEMBERS_LIMIT: usize = 90;
pub const MEMBERS_WINDOW: Duration = Duration::from_secs(900);
pub const SEARCH_LIMIT: usize = 10;
pub const SEARCH_WINDOW: Duration = Duration::from_secs(10);
/// Retries after the first 429 before giving up with `RateLimit`.
pub const MAX_RETRIES: u32 = 4;

/// Which windows a request draws from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteClass {
    Normal,
    Members,
    Search,
}

impl RouteClass {
    /// `path` is relative to `/1`, without the query string.
    pub fn for_path(path: &str) -> Self {
        let segs: Vec<&str> = path.trim_matches('/').split('/').collect();
        match segs.as_slice() {
            ["members", _] => Self::Members,
            ["search", ..] | ["membersSearch"] => Self::Search,
            _ => Self::Normal,
        }
    }
}

/// Sliding-window log: remembers when each of the last `limit` requests
/// was sent and waits until the oldest leaves the window.
#[derive(Debug)]
struct Window {
    limit: usize,
    span: Duration,
    sent: VecDeque<Instant>,
}

impl Window {
    fn new(limit: usize, span: Duration) -> Self {
        Self {
            limit: limit.max(1),
            span,
            sent: VecDeque::new(),
        }
    }

    /// When the next request may go out.
    fn next_slot(&mut self, now: Instant) -> Instant {
        while self
            .sent
            .front()
            .is_some_and(|t| now.duration_since(*t) >= self.span)
        {
            self.sent.pop_front();
        }
        if self.sent.len() < self.limit {
            now
        } else {
            self.sent[self.sent.len() - self.limit] + self.span
        }
    }
}

#[derive(Debug)]
struct State {
    token: Window,
    members: Window,
    search: Window,
    /// Set from `x-rate-limit-*-remaining` when Trello says we're nearly out.
    paused_until: Option<Instant>,
    token_remaining: Option<i64>,
    key_remaining: Option<i64>,
}

#[derive(Debug)]
pub struct RateLimiter {
    state: Mutex<State>,
    permits: Semaphore,
    backoff_base: Duration,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new(DEFAULT_CONCURRENCY)
    }
}

fn header_i64(headers: &HeaderMap, name: &str) -> Option<i64> {
    headers.get(name)?.to_str().ok()?.trim().parse().ok()
}

impl RateLimiter {
    pub fn new(concurrency: usize) -> Self {
        Self::with_limits(concurrency, TOKEN_LIMIT, TOKEN_WINDOW)
    }

    pub fn with_limits(concurrency: usize, token_limit: usize, token_window: Duration) -> Self {
        Self {
            state: Mutex::new(State {
                token: Window::new(token_limit, token_window),
                members: Window::new(MEMBERS_LIMIT, MEMBERS_WINDOW),
                search: Window::new(SEARCH_LIMIT, SEARCH_WINDOW),
                paused_until: None,
                token_remaining: None,
                key_remaining: None,
            }),
            permits: Semaphore::new(concurrency.max(1)),
            backoff_base: Duration::from_secs(1),
        }
    }

    /// Scale the 429 backoff (1 s, 2 s, 4 s, 8 s). Tests use milliseconds.
    pub fn with_backoff_base(mut self, base: Duration) -> Self {
        self.backoff_base = base;
        self
    }

    /// Bounded-concurrency permit for bulk operations.
    pub async fn permit(&self) -> SemaphorePermit<'_> {
        self.permits
            .acquire()
            .await
            .expect("rate limiter semaphore is never closed")
    }

    /// Wait until a request of `route` fits every window it draws from, then
    /// record it as sent.
    pub async fn acquire(&self, route: RouteClass) {
        loop {
            let at = {
                let mut s = self.state.lock().expect("rate limiter lock");
                let now = Instant::now();
                let mut at = s.token.next_slot(now);
                match route {
                    RouteClass::Members => at = at.max(s.members.next_slot(now)),
                    RouteClass::Search => at = at.max(s.search.next_slot(now)),
                    RouteClass::Normal => {}
                }
                if let Some(p) = s.paused_until {
                    if p > now {
                        at = at.max(p);
                    } else {
                        s.paused_until = None;
                    }
                }
                if at <= now {
                    s.token.sent.push_back(now);
                    match route {
                        RouteClass::Members => s.members.sent.push_back(now),
                        RouteClass::Search => s.search.sent.push_back(now),
                        RouteClass::Normal => {}
                    }
                    return;
                }
                at
            };
            tracing::debug!(wait = ?at.saturating_duration_since(Instant::now()), ?route, "pacing Trello requests");
            tokio::time::sleep_until(at).await;
        }
    }

    /// Read `x-rate-limit-api-{token,key}-remaining` / `-interval-ms`; when
    /// two or fewer requests remain, pause new requests for the interval.
    pub fn observe(&self, headers: &HeaderMap) {
        let token = header_i64(headers, "x-rate-limit-api-token-remaining");
        let key = header_i64(headers, "x-rate-limit-api-key-remaining");
        if token.is_none() && key.is_none() {
            return;
        }
        let mut s = self.state.lock().expect("rate limiter lock");
        s.token_remaining = token.or(s.token_remaining);
        s.key_remaining = key.or(s.key_remaining);
        let low = |remaining: Option<i64>, interval: &str| {
            remaining
                .filter(|r| *r <= 2)
                .map(|_| header_i64(headers, interval).unwrap_or(10_000).max(0) as u64)
        };
        let wait = low(token, "x-rate-limit-api-token-interval-ms")
            .into_iter()
            .chain(low(key, "x-rate-limit-api-key-interval-ms"))
            .max();
        if let Some(ms) = wait {
            let until = Instant::now() + Duration::from_millis(ms.min(10_000));
            s.paused_until = Some(s.paused_until.map_or(until, |p| p.max(until)));
        }
    }

    /// Last seen `(token, key)` remaining counts.
    pub fn remaining(&self) -> (Option<i64>, Option<i64>) {
        let s = self.state.lock().expect("rate limiter lock");
        (s.token_remaining, s.key_remaining)
    }

    /// Backoff before retry `attempt` (1-based): base × 2^(attempt-1), capped
    /// at the 10 s window.
    pub fn backoff(&self, attempt: u32) -> Duration {
        (self.backoff_base * 2u32.saturating_pow(attempt.saturating_sub(1)).min(16))
            .min(TOKEN_WINDOW.max(self.backoff_base))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;
    use std::sync::Arc;

    #[test]
    fn route_classes() {
        assert_eq!(RouteClass::for_path("/members/me"), RouteClass::Members);
        assert_eq!(RouteClass::for_path("/members/abc123"), RouteClass::Members);
        assert_eq!(
            RouteClass::for_path("/members/me/boards"),
            RouteClass::Normal
        );
        assert_eq!(
            RouteClass::for_path("/members/me/cards"),
            RouteClass::Normal
        );
        assert_eq!(RouteClass::for_path("/search"), RouteClass::Search);
        assert_eq!(RouteClass::for_path("/search/members"), RouteClass::Search);
        assert_eq!(RouteClass::for_path("/boards/x/cards"), RouteClass::Normal);
    }

    fn max_in_window(times: &[Instant], span: Duration) -> usize {
        times
            .iter()
            .map(|start| {
                times
                    .iter()
                    .filter(|t| **t >= *start && t.duration_since(*start) < span)
                    .count()
            })
            .max()
            .unwrap_or(0)
    }

    #[tokio::test(start_paused = true)]
    async fn never_exceeds_90_per_rolling_10s() {
        let l = RateLimiter::default();
        let mut times = Vec::new();
        for i in 0..200 {
            l.acquire(RouteClass::Normal).await;
            times.push(Instant::now());
            if i % 7 == 0 {
                tokio::time::advance(Duration::from_millis(30)).await;
            }
        }
        assert_eq!(max_in_window(&times, TOKEN_WINDOW), 90);
        // 200 requests need at least two full windows.
        assert!(times[199].duration_since(times[0]) >= Duration::from_secs(20));
    }

    #[tokio::test(start_paused = true)]
    async fn concurrent_callers_share_the_budget() {
        let l = Arc::new(RateLimiter::default());
        let start = Instant::now();
        let tasks: Vec<_> = (0..120)
            .map(|_| {
                let l = l.clone();
                tokio::spawn(async move {
                    l.acquire(RouteClass::Normal).await;
                    Instant::now()
                })
            })
            .collect();
        let mut times = Vec::new();
        for t in tasks {
            times.push(t.await.unwrap());
        }
        assert_eq!(max_in_window(&times, TOKEN_WINDOW), 90);
        assert!(*times.iter().max().unwrap() >= start + TOKEN_WINDOW);
    }

    #[tokio::test(start_paused = true)]
    async fn members_bucket_only_for_top_level_member_routes() {
        let l = RateLimiter::default();
        let start = Instant::now();
        for _ in 0..MEMBERS_LIMIT {
            l.acquire(RouteClass::for_path("/members/me")).await;
        }
        // Nested member routes aren't held back by the members budget…
        l.acquire(RouteClass::for_path("/members/me/boards")).await;
        assert!(Instant::now() - start < MEMBERS_WINDOW);
        // …but the 91st /members/me waits for the 900 s window.
        l.acquire(RouteClass::for_path("/members/me")).await;
        assert!(Instant::now() - start >= MEMBERS_WINDOW);
    }

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(*k, HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[tokio::test(start_paused = true)]
    async fn low_remaining_pauses_for_the_interval() {
        let l = RateLimiter::default();
        l.observe(&headers(&[
            ("x-rate-limit-api-token-remaining", "50"),
            ("x-rate-limit-api-key-remaining", "250"),
        ]));
        assert_eq!(l.remaining(), (Some(50), Some(250)));
        let start = Instant::now();
        l.acquire(RouteClass::Normal).await;
        assert_eq!(Instant::now(), start);

        l.observe(&headers(&[
            ("x-rate-limit-api-token-remaining", "1"),
            ("x-rate-limit-api-token-interval-ms", "4000"),
        ]));
        l.acquire(RouteClass::Normal).await;
        assert!(Instant::now() - start >= Duration::from_secs(4));
    }

    #[test]
    fn backoff_doubles_and_caps() {
        let l = RateLimiter::default();
        let secs: Vec<u64> = (1..=6).map(|a| l.backoff(a).as_secs()).collect();
        assert_eq!(secs, vec![1, 2, 4, 8, 10, 10]);
    }
}
