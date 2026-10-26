use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use crate::engine::{Engine, LineResult, Sheet};

const SLICE: Duration = Duration::from_millis(100);

struct Job {
    id: u64,
    text: String,
}

pub struct Done {
    pub text: String,
    pub results: Vec<LineResult>,
    pub complete: bool,
}

pub struct Calculator {
    jobs: Sender<Job>,
    done: Receiver<Done>,
    latest: Arc<AtomicU64>,
}

impl Calculator {
    pub fn new(engine: Engine, wake: impl Fn() + Send + 'static) -> Self {
        let (jobs, inbox) = channel::<Job>();
        let (outbox, done) = channel::<Done>();
        let latest = Arc::new(AtomicU64::new(0));
        let newest = latest.clone();
        std::thread::Builder::new()
            .name("calc".into())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let mut sheet = Sheet::default();
                while let Ok(mut job) = inbox.recv() {
                    while let Ok(next) = inbox.try_recv() {
                        job = next;
                    }
                    loop {
                        let newer = || newest.load(Ordering::Relaxed) != job.id;
                        let until = Instant::now() + SLICE;
                        let stop = || newer() || Instant::now() > until;
                        let results = engine.eval_sheet_until(&job.text, &mut sheet, stop);
                        if results.is_none() && newer() {
                            break;
                        }
                        let complete = results.is_some();
                        let out = Done {
                            text: job.text.clone(),
                            results: results.unwrap_or_else(|| sheet.done().to_vec()),
                            complete,
                        };
                        if outbox.send(out).is_err() {
                            return;
                        }
                        wake();
                        if complete {
                            break;
                        }
                    }
                }
            })
            .expect("start the calculation thread");
        Self { jobs, done, latest }
    }

    pub fn request(&self, text: &str) {
        let id = self.latest.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = self.jobs.send(Job {
            id,
            text: text.to_string(),
        });
    }

    pub fn take(&self) -> Option<Done> {
        self.done.try_iter().last()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Rates;

    fn engine() -> Engine {
        Engine::new(Rates::fixed(&[("USD", 1.0)])).with_clock(crate::engine::monotonic_ms)
    }

    fn wait(calc: &Calculator) -> Done {
        let start = Instant::now();
        loop {
            if let Some(done) = calc.take().filter(|d| d.complete) {
                return done;
            }
            assert!(start.elapsed() < Duration::from_secs(30), "no answers");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn never_makes_the_caller_wait() {
        let calc = Calculator::new(engine(), || {});
        let slow = vec!["100000!"; 20].join("\n");
        let start = Instant::now();
        calc.request(&slow);
        calc.request("2 + 2");
        assert!(
            start.elapsed() < Duration::from_millis(50),
            "request blocked"
        );
        let mut last = wait(&calc);
        while let Some(more) = calc.take() {
            last = more;
        }
        std::thread::sleep(Duration::from_millis(300));
        if let Some(more) = calc.take() {
            last = more;
        }
        assert_eq!(last.results.len(), 1);
        assert_eq!(last.results[0].display, "4");
    }

    #[test]
    fn answers_match_calculating_from_scratch() {
        let calc = Calculator::new(engine(), || {});
        let reference = engine();
        let mut text = String::new();
        for line in [
            "rent = $1000",
            "rent + 5",
            "",
            "tax = 8%",
            "$100 + tax",
            "total",
            "x = 3",
            "x * 2",
        ] {
            text.push_str(line);
            calc.request(&text);
            let done = wait(&calc);
            let fresh = reference.eval_document(&text);
            let got: Vec<_> = done.results.iter().map(|r| r.display.clone()).collect();
            let want: Vec<_> = fresh.iter().map(|r| r.display.clone()).collect();
            assert_eq!(got, want, "{text:?}");
            text.push('\n');
        }
    }

    #[test]
    fn answers_arrive_part_way_through() {
        let calc = Calculator::new(engine(), || {});
        let text = ["1 + 1", "100000!", "100000!", "100000!", "100000!", "2 + 2"].join("\n");
        calc.request(&text);
        let start = Instant::now();
        let mut part = None;
        let done = loop {
            assert!(start.elapsed() < Duration::from_secs(30), "no answers");
            match calc.take() {
                Some(d) if d.complete => break d,
                Some(d) => part = part.or(Some(d)),
                None => std::thread::sleep(Duration::from_millis(5)),
            }
        };
        let part = part.expect("answers before the slow lines were done");
        assert_eq!(part.text, text);
        assert!(part.results.len() < 6);
        assert_eq!(part.results[0].display, "2");
        assert_eq!(done.results.len(), 6);
        assert_eq!(done.results[5].display, "4");
    }
}
