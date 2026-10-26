use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::engine::Rates;

const RATES_URL: &str = "https://open.er-api.com/v6/latest/USD";
const MAX_AGE: Duration = Duration::from_secs(12 * 60 * 60);
const RETRY_AFTER: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Default)]
pub struct RatesFetcher {
    table: Rates,
    fetched_at: Arc<AtomicU64>,
    tried_at: Arc<AtomicU64>,
    fetching: Arc<AtomicBool>,
}

impl RatesFetcher {
    pub fn table(&self) -> Rates {
        self.table.clone()
    }

    pub fn version(&self) -> u64 {
        self.table.version()
    }

    pub fn refresh_if_stale(&self, on_update: impl Fn() + Send + 'static) {
        let now = now_secs();
        let stale = now.saturating_sub(self.fetched_at.load(Ordering::Relaxed)) > MAX_AGE.as_secs();
        let retry =
            now.saturating_sub(self.tried_at.load(Ordering::Relaxed)) > RETRY_AFTER.as_secs();
        if !stale || !retry || self.fetching.swap(true, Ordering::Relaxed) {
            return;
        }
        self.tried_at.store(now, Ordering::Relaxed);
        let rates = self.clone();
        std::thread::spawn(move || {
            if let Ok(fresh) = fetch() {
                rates.table.set(fresh);
                rates.fetched_at.store(now_secs(), Ordering::Relaxed);
                on_update();
            }
            rates.fetching.store(false, Ordering::Relaxed);
        });
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn fetch() -> Result<HashMap<String, f64>, Box<dyn std::error::Error>> {
    let body = ureq::get(RATES_URL).call()?.body_mut().read_to_string()?;
    let value: serde_json::Value = serde_json::from_str(&body)?;
    let mut out = HashMap::new();
    if let Some(rates) = value.get("rates").and_then(|rates| rates.as_object()) {
        for (code, amount) in rates {
            if let Some(amount) = amount.as_f64() {
                out.insert(code.clone(), amount);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_shares_one_arc_across_clones() {
        let fetcher = RatesFetcher::default();
        let engine_side = fetcher.table();
        let before = engine_side.version();
        fetcher
            .table()
            .set(HashMap::from([("EUR".to_string(), 0.9)]));
        assert!(engine_side.knows("EUR"));
        assert_ne!(engine_side.version(), before);
        assert_eq!(fetcher.version(), engine_side.version());
    }

    #[test]
    fn a_window_left_open_tries_again_after_the_retry_gap() {
        let fetcher = RatesFetcher::default();
        fetcher.tried_at.store(now_secs(), Ordering::Relaxed);
        fetcher.refresh_if_stale(|| {});
        assert!(!fetcher.fetching.load(Ordering::Relaxed));
    }
}
