use std::collections::HashSet;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use regex::Regex;

pub mod complete;
pub mod currency;
pub mod extras;
pub mod markdown;
pub mod peg;
mod rates;
pub mod rename;
pub mod time;

pub use rates::Rates;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Empty,
    Header,
    Comment,
    Value,
    Error,
}

#[derive(Clone, Debug)]
pub struct LineResult {
    pub kind: Kind,
    pub display: String,
    pub color: Option<[u8; 3]>,
    pub note: Option<String>,
    pub hint: bool,
    pub clock: bool,
    pub stale: bool,
}

#[derive(Clone)]
struct State {
    ctx: fend_core::Context,
    block: Vec<Term>,
    prev: Option<String>,
    vars: HashSet<String>,
    percents: Vec<(String, Regex, String)>,
}

#[derive(Clone)]
struct Term {
    var: String,
    plain: bool,
    aside: bool,
}

#[derive(Clone, Default)]
pub struct Sheet {
    lines: Vec<String>,
    results: Vec<LineResult>,
    after: Vec<State>,
    rates_version: u64,
    defs: HashSet<String>,
}

impl Sheet {
    pub fn done(&self) -> &[LineResult] {
        &self.results
    }
}

impl LineResult {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            display: String::new(),
            color: None,
            note: None,
            hint: false,
            clock: false,
            stale: false,
        }
    }

    fn value(display: String) -> Self {
        Self {
            kind: Kind::Value,
            display,
            color: None,
            note: None,
            hint: false,
            clock: false,
            stale: false,
        }
    }

    fn error(note: Option<&str>) -> Self {
        Self {
            note: note.map(str::to_string),
            ..Self::new(Kind::Error)
        }
    }

    fn hint(note: String) -> Self {
        Self {
            hint: true,
            note: Some(note),
            ..Self::new(Kind::Error)
        }
    }
}

fn warm_up() {
    static WARM: Once = Once::new();
    WARM.call_once(|| {
        let engine = Engine {
            rates: Rates::fixed(&[("USD", 1.0), ("EUR", 0.9)]),
            clock: None,
        };
        engine.eval_document("x = 2 km in m\n3 ft in cm\n$5 + 10% in EUR\nx * 2\ntotal");
    });
}

const LINE_MS: f64 = 150.0;
const MAX_CHECKS: u64 = 2_000_000;
const MAX_CHECKS_TIMED: u64 = 200_000_000;

struct Budget {
    checks: AtomicU64,
    max_checks: u64,
    clock: Option<fn() -> f64>,
    deadline: f64,
    stopped: AtomicBool,
    error: Mutex<Option<String>>,
}

impl Budget {
    fn new(clock: Option<fn() -> f64>, ms: f64) -> Self {
        Self {
            checks: AtomicU64::new(0),
            max_checks: if clock.is_some() {
                MAX_CHECKS_TIMED
            } else {
                MAX_CHECKS
            },
            clock,
            deadline: clock.map_or(0.0, |now| now() + ms),
            stopped: AtomicBool::new(false),
            error: Mutex::new(None),
        }
    }

    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }

    fn evaluate(
        &self,
        input: &str,
        ctx: &mut fend_core::Context,
    ) -> Result<fend_core::FendResult, String> {
        if self.stopped() {
            return Err("interrupted".into());
        }
        let r = fend_core::evaluate_with_interrupt(input, ctx, self);
        if let Err(e) = &r {
            *self.error.lock().unwrap() = Some(e.clone());
        }
        r
    }

    fn note(&self) -> Option<&'static str> {
        if self.stopped() {
            return Some("Too big to work out");
        }
        let error = self.error.lock().unwrap();
        let error = error.as_deref()?;
        if error.contains("division by zero") || error.contains("modulo by zero") {
            Some("Can't divide by zero")
        } else if error.contains("modulo is only supported for positive integers") {
            Some("mod needs whole numbers, 0 or more")
        } else if error.contains("are incompatible") {
            Some("Units don't match")
        } else if error.contains("must lie in the interval") || error.contains("is undefined") {
            Some("Not defined")
        } else {
            None
        }
    }
}

impl fend_core::Interrupt for Budget {
    fn should_interrupt(&self) -> bool {
        let n = self.checks.fetch_add(1, Ordering::Relaxed);
        let over = n >= self.max_checks
            || (n.is_multiple_of(128) && self.clock.is_some_and(|now| now() > self.deadline));
        if over {
            self.stopped.store(true, Ordering::Relaxed);
        }
        over
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn monotonic_ms() -> f64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_secs_f64()
        * 1000.0
}

pub(crate) fn looks_like_math(body: &str, vars: &HashSet<String>) -> bool {
    let body = body.trim();
    if body.is_empty() {
        return false;
    }
    if aggregate(body).is_some() || ASSIGN.is_match(body) || PREV.is_match(body) {
        return true;
    }
    if body.contains([
        '$', '£', '€', '¥', '₹', '₩', '₽', '₺', '%', '^', '*', '/', '+', '=',
    ]) {
        return true;
    }
    if body.contains(" - ") || body.contains(" – ") || body.contains(" — ") {
        return true;
    }
    if body
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit() || matches!(c, '(' | '.' | '-'))
    {
        return true;
    }

    if body.contains('(') && body.chars().any(|c| c.is_ascii_digit()) {
        return true;
    }
    if LONE_WORD.is_match(body)
        && (vars.contains(body) || matches!(body, "pi" | "π" | "e" | "tau" | "τ" | "phi" | "φ"))
    {
        return true;
    }
    crate::engine::time::eval(body).is_some()
        || crate::engine::time::impossible_date(body)
        || crate::engine::extras::eval(body).is_some()
}

fn shown_math(result: &LineResult) -> bool {
    match result.kind {
        Kind::Value => !result.display.is_empty(),
        Kind::Error => result.note.is_some() && !result.hint,
        _ => false,
    }
}

#[derive(Clone)]
pub struct Engine {
    clock: Option<fn() -> f64>,
    rates: Rates,
}

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).unwrap());
    };
}

pub const LABEL_PATTERN: &str = r"^\s*([A-Za-z][A-Za-z0-9 '\-_&/]*?)\s*:\s+(.*)$";
re!(LABEL, LABEL_PATTERN);
re!(QUOTED, r#""[^"]*""#);
re!(ASSIGN, r"^\s*([A-Za-z_][A-Za-z0-9_]*)\s*=\s*([^=].*)$");

re!(PCT_ON, r"(?i)^(.+?)\s*%\s+(?:[a-z]+\s+)?on\s+(.+)$");
re!(PCT_OFF, r"(?i)^(.+?)\s*%\s+off\s+(.+)$");
re!(PCT_OF_WHAT, r"(?i)^(.+?)\s*%\s+of\s+what\s+is\s+(.+)$");
re!(AS_PCT, r"(?i)^(.+?)\s+as\s+(?:a\s+)?%\s+(?:of\s+)?(.+)$");
re!(IS_WHAT_PCT, r"(?i)^(.+?)\s+is\s+what\s+%\s+of\s+(.+)$");
re!(IS_PCT_OF_WHAT, r"(?i)^(.+?)\s+is\s+(.+?)\s*%\s+of\s+what$");

re!(
    PLUS_MINUS_PCT_STEP,
    r"^([^%]+?)\s*([+-])\s*(\d[\d,]*(?:\.\d+)?\s*%(?:\s*[*/]\s*\d[\d,]*(?:\.\d+)?)*)"
);

re!(
    BARE_NUMBER,
    r"(^|[+\-(]\s*)(\d[\d,]*(?:\.\d+)?)(\s*(?:[+\-)]|$))"
);

re!(
    CURRENCY_VS_PLAIN,
    r"cannot convert from (\S+) to (\S+): units '(\w+)' and '(\w+)' are incompatible"
);

re!(
    DIVISOR,
    r"([A-Za-z)]\s*)/\s*(\(?\d[\d,]*(?:\.\d+)?(?:[eE][+-]?\d+)?\)?\s*)([A-Za-df-zA-DF-Z_][A-Za-z_]*(?:\^\d+)?(?:/[A-Za-z][A-Za-z_]*(?:\^\d+)?)*)"
);
re!(TIMES_X, r"(\d+\)?)(\s*)([x×])(\s*)(\d|\()");
re!(
    SYMBOL_NUM,
    r"([€¥₹₩₽₺])\s*(\d[\d,]*(?:\.\d+)?(?:[eE][+-]?\d+)?)\b"
);

re!(
    PREFIXED_DOLLAR,
    r"\b(US|CA|C|AU|A|NZ|HK|SG|S)\$\s*(\d[\d,]*(?:\.\d+)?)"
);

re!(
    DOT_THOUSANDS,
    r"(^|[^\d.,])(\d{1,3}(?:\.\d{3})+),(\d+)($|[^\d.,])"
);
re!(
    DOT_MILLIONS,
    r"(^|[^\d.,])(\d{1,3}(?:\.\d{3}){2,})($|[^\d.,])"
);

re!(SYMBOL_MINUS, r"(^|[^\w)])([$£€¥₹])\s*-\s*(\d)");

re!(
    DECIMAL_COMMA,
    r"(^|[^\d,.])(\d+),(\d{1,2}|\d{4,})($|[^\d,])"
);

re!(PCT_SHARE, r"(\d[\d.]*)\s*%(\s*\*)");

re!(MOD_WRITTEN, r"(?i)\bmod\b|\d\s*%\s*[\d(]");
re!(TIMES_PCT, r"(\d\)?\s*[*/]\s*)(\d[\d.]*)\s*%");
re!(MONEY_PREFIX, r"[$£]\s*\d[\d,]*(?:\.\d+)?(?:[eE][+-]?\d+)?");
re!(PREV, r"(?i)\bprev\b");
re!(LETTERS, r"[A-Za-z]+");

re!(
    MONEY_SCALE,
    r"(?i)([$£€¥₹₩₽₺])\s*(\d[\d,]*(?:\.\d+)?)\s*(mm|bn|k|m|b)\b"
);
re!(TOTAL_WORD, r"(?i)\b(?:total|sum|average|avg|mean)\b");
re!(CONVERSION_START, r"(?i)^(?:in|to|as)\b");

re!(LONE_WORD, r"^[A-Za-zπτφ]+$");
re!(IDENT, r"\b[A-Za-z_][A-Za-z0-9_]*\b");

re!(VERSION_WORD, r"(?i)\bversion\b");

re!(
    LAKH_CRORE,
    r"(?i)\b(\d[\d,]*(?:\.\d+)?)\s*(lakhs?|lacs?|crores?)\b"
);

re!(INCHES, r"(\d)\s*in(\s*(?:$|[+\-*/),]|(?:in|to|as)\b))");

re!(PCT_GLUED, r"(?i)%(off|on|of)\b");

re!(
    PLUS_MINUS_PCT_OF,
    r"(?i)([+\-]\s*)(\d[\d,]*(?:\.\d+)?)\s*%\s+of\s+(\(?\$?\d[\d,]*(?:\.\d+)?\)?(?:\s+[A-Z]{3}\b)?)"
);

re!(PERCENT_WORD, r"(?i)(\d)\s*(?:percent|per\s+cent)\b");

re!(HALF, r"(?i)\b(?:a\s+)?half(?:\s+of)?\s+");
re!(THIRD, r"(?i)\b(?:a\s+|one\s+)?third(?:\s+of)?\s+");
re!(QUARTER, r"(?i)\b(?:a\s+|one\s+)?quarter(?:\s+of)?\s+");
re!(TWICE, r"(?i)\b(?:twice|double)\s+");
re!(TRIPLE, r"(?i)\btriple\s+");

re!(
    A_DAY,
    r"(?i)(\d[\d,.]*\)?)\s+(?:a|an|per)\s+(day|week|month|year|hour|minute|night|person|head)s?\b"
);

re!(
    SPLIT,
    r"(?i)^(?:split\s+(.+?)|(.+?)\s+split)\s+(?:(\d+)\s+ways|(?:between|among|across|by)\s+(\d+)(?:\s+(?:people|persons|friends|of\s+us))?)$"
);

re!(
    TRANSFER,
    r"(?i)^(.*\d.*?\b(?:[kmgtpe]i?b|[kmgtpe]?bytes?|[kmgtpe]?bits?))\s+(?:at|over|@)\s+(.*\d.*?(?:bps|b/s|bits?/s|bytes?/s|per\s+second))$"
);

re!(AT_PRICE, r"(?i)(\d.*?)\s+at\s+([$£€¥₹(]|\d)");

re!(
    RATE_DIVISOR,
    r"/\s*([$£€¥₹]\s*\d[\d,]*(?:\.\d+)?[kKM]?|\d[\d,]*(?:\.\d+)?[kKM])\s*(?:(?i:per)|/)\s*((?i)months?|years?|weeks?|days?|hours?|minutes?|nights?|people|persons?|heads?|mo|yr)\b"
);

re!(
    RATE_IN_TIME,
    r"(?i)((?:per|/)\s*(?:months?|years?|weeks?|days?|mo|yr))\s+(?:in|to|as)\s+(years?|months?|weeks?|days?|yr|mo|annually)\b"
);

re!(SCALE_SUFFIX, r"(^|[^\w])(\d[\d,]*(?:\.\d+)?)([kKM])\b");

re!(
    AT_PRE,
    r"(?i)^\s*(.+?)\s+at\s+(?:an?\s+)?(.+?)\s+pre(?:-?money|\s+money)?\s*$"
);

re!(
    AT_POST,
    r"(?i)^\s*(.+?)\s+at\s+(?:an?\s+)?(.+?)\s+post(?:-?money|\s+money)?\s*$"
);
re!(
    POST_AT,
    r"(?i)^\s*(.+?)\s+post(?:-?money|\s+money)?\s+at\s+(?:an?\s+)?(.+?)\s*$"
);

re!(
    PERIOD_OF_RATE,
    r"(?i)((?:per|/)\s*(?:months?|years?|weeks?|days?|mo|yr))\s+(monthly|annually)\b"
);
re!(
    PERIOD_TARGET,
    r"(?i)\b(in|to|as)\s+(monthly|annually|MRR|ARR)\b"
);
re!(
    PERIOD_WORD,
    r"(?i)(\d[\d,.]*\)?(?:\s+[A-Z]{3})?|\)(?:\s+[A-Z]{3})?)\s+(monthly|annually|MRR|ARR)\b"
);

re!(CLOCK_WORD, r"(?i)\b\d{1,2}(?::\d{2})?\s?[ap]\.?m\b");

re!(
    LABEL_NUMBER,
    r"^[A-Za-z][^\d=]*?\d[\d,.]*\s+(\(?[$£€¥₹]?\s*\d.*)$"
);

re!(SIDE_BY_SIDE, r"\d[\d,.]*\)?\s+(\(?[$£€¥₹]?\s*\d)");
re!(UNKNOWN_IDENT, r"unknown identifier '([^']+)'");
re!(
    BASE_CONV,
    r"(?i)\b(in|to|as)\s+(hex|hexadecimal|binary|bin|octal|oct|base\s*\d+)\b"
);
re!(WORD, r"\b[A-Za-z]{3}\b");

re!(NUM_TOKEN, r"^(-?)([$£€¥₹]?)(-?)(\d+)(?:\.(\d+))?$");

const WORD_OPS: &[(&str, &str)] = &[
    (r"(?i)\bmultiplied\s+by\b", "*"),
    (r"(?i)\bdivided\s+by\b", "/"),
    (r"(?i)\btimes\b", "*"),
    (r"(?i)\bplus\b", "+"),
    (r"(?i)\bminus\b", "-"),
];

const UNIT_ALIASES: &[(&str, &str)] = &[
    (r"(?i)\bnautical\s+miles?\b", "nautical_mile"),
    (r"(?i)\b(?:fluid\s+ounces?|fl\.?\s*oz)\b", "floz"),
    (r"(?i)\bshort\s+tons?\b", "short_ton"),
    (r"(?i)\bmetric\s+(?:tons?|tonnes?)\b", "tonne"),
    (r"(?i)\bsq\.?\s*(?:ft|feet|foot)\b", "ft^2"),
    (r"(?i)\bsq\.?\s*(?:m|meters?|metres?)\b", "m^2"),
    (r"(?i)\bus\s+gallons?\b", "gallon"),
];

const IMPERIAL: &[(&str, &str, &str)] = &[
    ("gallon", "4.54609 liters", "imperial gallons"),
    ("quart", "1.1365225 liters", "imperial quarts"),
    ("pint", "568.26125 ml", "imperial pints"),
    ("floz", "28.4130625 ml", "imperial fl oz"),
];

re!(
    IMPERIAL_UNIT,
    r"(?i)\b(?:imperial|uk|british)\s+(gallons?|quarts?|pints?|fluid\s+ounces?|fl\.?\s*oz)\b"
);
re!(
    IMPERIAL_TARGET,
    r"(?i)^(.*\S)\s+(?:in|to|as)\s+(?:imperial|uk|british)\s+(gallons?|quarts?|pints?|fluid\s+ounces?|fl\.?\s*oz)\s*$"
);

re!(CODE_FIRST, r"\b([A-Z]{3})\s+(\d[\d,]*(?:\.\d+)?)\b");

fn imperial(word: &str) -> &'static (&'static str, &'static str, &'static str) {
    let w = word.to_ascii_lowercase();
    IMPERIAL
        .iter()
        .find(|(key, ..)| w.starts_with(&key[..3]) || (*key == "floz" && w.starts_with('f')))
        .unwrap_or(&IMPERIAL[0])
}

fn imperial_target(line: &str) -> Option<(String, &'static str)> {
    let line = match LABEL.captures(line) {
        Some(c) => c.get(2).unwrap().as_str(),
        None => line,
    };
    let c = IMPERIAL_TARGET.captures(line)?;
    let (_, size, name) = imperial(&c[2]);
    Some((format!("({}) / ({size})", &c[1]), name))
}

static UNIT_ALIAS_RES: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    UNIT_ALIASES
        .iter()
        .map(|(p, r)| (Regex::new(p).unwrap(), *r))
        .collect()
});

static WORD_OP_RES: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    WORD_OPS
        .iter()
        .map(|(p, r)| (Regex::new(p).unwrap(), *r))
        .collect()
});

const SYMBOL_CODES: &[(char, &str)] = &[
    ('€', "EUR"),
    ('¥', "JPY"),
    ('₹', "INR"),
    ('₩', "KRW"),
    ('₽', "RUB"),
    ('₺', "TRY"),
];

fn normalize_typing(line: &str) -> std::borrow::Cow<'_, str> {
    let odd = line.chars().any(|c| typing_char(c).is_some());
    if !odd {
        return std::borrow::Cow::Borrowed(line);
    }
    std::borrow::Cow::Owned(
        line.chars()
            .filter_map(|c| match c {
                '\u{feff}' => None,
                _ => Some(typing_char(c).unwrap_or(c)),
            })
            .collect(),
    )
}

fn typing_char(c: char) -> Option<char> {
    Some(match c {
        '\u{00a0}' | '\u{2007}' | '\u{202f}' | '\u{2008}' | '\u{2009}' | '\u{200a}' => ' ',
        '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}' => '\'',
        '\u{201c}' | '\u{201d}' | '\u{201e}' | '\u{201f}' => '"',
        '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2212}' => '-',
        _ => return None,
    })
}

impl Engine {
    pub fn new(rates: Rates) -> Self {
        Self { rates, clock: None }
    }

    pub fn with_clock(mut self, clock: fn() -> f64) -> Self {
        self.clock = Some(clock);
        self
    }

    fn context(&self) -> fend_core::Context {
        let mut ctx = fend_core::Context::new();
        ctx.set_exchange_rate_handler_v2(self.rates.clone());
        ctx
    }

    pub fn eval_document(&self, text: &str) -> Vec<LineResult> {
        let defs = defined_variables(text);
        let mut st = self.fresh();
        text.split('\n')
            .enumerate()
            .map(|(i, line)| self.eval_in(&mut st, i, line, &defs))
            .collect()
    }

    pub fn eval_sheet_until(
        &self,
        text: &str,
        sheet: &mut Sheet,
        stop: impl Fn() -> bool,
    ) -> Option<Vec<LineResult>> {
        let lines: Vec<&str> = text.split('\n').collect();
        let defs = defined_variables(text);
        let defs_changed = sheet.defs != defs;
        sheet.defs = defs.clone();
        let version = self.rates.version();

        let same = if sheet.rates_version == version && !defs_changed {
            lines
                .iter()
                .zip(&sheet.lines)
                .take_while(|(a, b)| **a == b.as_str())
                .count()
        } else {
            0
        };
        sheet.lines.truncate(same);
        sheet.results.truncate(same);
        sheet.after.truncate(same);
        sheet.rates_version = version;

        for (line, result) in sheet.lines.iter().zip(sheet.results.iter_mut()) {
            if result.clock
                && let Some(now) = crate::engine::time::eval(markdown::math_body(line).trim())
            {
                result.display = now;
            }
        }
        let mut st = sheet.after.last().cloned().unwrap_or_else(|| self.fresh());
        for (i, line) in lines.iter().enumerate().skip(same) {
            if stop() {
                return None;
            }
            let result = self.eval_in(&mut st, i, line, &defs);
            sheet.lines.push((*line).to_string());
            sheet.results.push(result);
            sheet.after.push(st.clone());
        }
        Some(sheet.results.clone())
    }

    fn fresh(&self) -> State {
        State {
            ctx: self.context(),
            block: Vec::new(),
            prev: None,
            vars: HashSet::new(),
            percents: Vec::new(),
        }
    }

    fn eval_in(&self, st: &mut State, i: usize, line: &str, defs: &HashSet<String>) -> LineResult {
        let normalized = normalize_typing(line);

        let line = normalized.as_ref();
        let class = markdown::classify(line);
        match class.form {
            markdown::Form::Blank => {
                st.block.clear();
                return LineResult::new(Kind::Empty);
            }
            markdown::Form::Comment => return LineResult::new(Kind::Comment),

            markdown::Form::Table => return LineResult::new(Kind::Error),
            _ => {}
        }
        let trimmed = class.body.trim();
        let prefixed = !matches!(class.form, markdown::Form::Text);

        if prefixed && (trimmed.is_empty() || !looks_like_math(trimmed, &st.vars)) {
            if matches!(class.form, markdown::Form::Heading(_)) {
                st.block.clear();
                return LineResult::new(Kind::Header);
            }
            return LineResult::new(Kind::Error);
        }
        let result = self.eval_math(st, i, trimmed, defs);
        if matches!(class.form, markdown::Form::Heading(_)) && !shown_math(&result) {
            st.block.clear();
            return LineResult::new(Kind::Header);
        }
        result
    }

    fn eval_math(
        &self,
        st: &mut State,
        i: usize,
        trimmed: &str,
        defs: &HashSet<String>,
    ) -> LineResult {
        if let Some(date) = crate::engine::time::eval(trimmed) {
            return LineResult {
                clock: true,
                ..LineResult::value(date)
            };
        }
        if crate::engine::time::impossible_date(trimmed) {
            return LineResult::error(Some("Not a date"));
        }
        if let Some(extra) = crate::engine::extras::eval(trimmed) {
            return LineResult {
                color: extra.color,
                ..LineResult::value(extra.display)
            };
        }

        if CLOCK_WORD.is_match(trimmed) || trimmed.len() > 2000 {
            return LineResult::new(Kind::Error);
        }
        if st.prev.is_none() && PREV.is_match(trimmed) && !ASSIGN.is_match(trimmed) {
            return LineResult::new(Kind::Error);
        }
        if LONE_WORD.is_match(trimmed)
            && !st.vars.contains(trimmed)
            && aggregate(trimmed).is_none()
            && !PREV.is_match(trimmed)
            && !matches!(trimmed, "pi" | "π" | "e" | "tau" | "τ" | "phi" | "φ")
        {
            return match self.name_hint(trimmed, &st.vars, defs) {
                Some(h) if h.starts_with("Unknown name") && self.fend_knows_word(trimmed) => {
                    LineResult::new(Kind::Error)
                }
                Some(h) => LineResult::hint(h),
                None => LineResult::new(Kind::Error),
            };
        }

        if self.clock.is_some() {
            warm_up();
        }
        let budget = Budget::new(self.clock, LINE_MS);
        let var = format!("_l{i}");

        let whole = match ASSIGN.captures(trimmed) {
            Some(c) => aggregate(&c[2]).map(|avg| (Some(c[1].to_string()), avg)),
            None => aggregate(trimmed).map(|avg| (None, avg)),
        };
        let mut is_total = whole.is_some();
        let target = imperial_target(trimmed);
        let source = target.as_ref().map_or(trimmed, |(expr, _)| expr.as_str());

        let mut peg: Option<crate::engine::peg::Program> = None;
        let result = match whole {
            Some((name, avg)) => {
                total_into(&mut st.ctx, &budget, &var, &st.block, avg, name.as_deref())
            }
            None => {
                let mut source = source.to_string();
                for (_, re, pct) in &st.percents {
                    source = re.replace_all(&source, format!("${{1}}{pct}")).into_owned();
                }

                let (source, used) = totals_inside(&source, &st.vars, i);
                is_total |= used.iter().any(Option::is_some);
                let mut vars = std::borrow::Cow::Borrowed(&st.vars);
                for name in used.iter().flatten() {
                    vars.to_mut().insert(name.clone());
                }
                peg = crate::engine::peg::parse_line(&source, st.prev.as_deref(), &vars);
                let expr = match peg.as_ref() {
                    Some(program) => program.fend.clone(),
                    None => self.currency_first(&rewrite(&source, st.prev.as_deref(), &vars)),
                };
                let expr = self.drop_name_collisions(&expr, &vars);
                if expr.trim().is_empty() {
                    return LineResult::new(Kind::Comment);
                }
                let totals_ok = used.iter().zip([false, true]).all(|(name, avg)| {
                    name.as_ref().is_none_or(|name| {
                        total_into(&mut st.ctx, &budget, name, &st.block, avg, None).is_some()
                    })
                });
                let mut eval = |expr: &str| {
                    eval_forgiving(&mut st.ctx, &budget, &var, expr)
                        .or_else(|| self.with_currency(&mut st.ctx, &budget, &var, expr))
                };
                match totals_ok.then(|| eval(&expr)).flatten() {
                    Some(raw) if self.money_times_unit(&raw) => {
                        let prose = self.units_as_prose(&expr, &vars);
                        eval(&prose).filter(|raw| !self.money_times_unit(raw))
                    }
                    Some(raw) => Some(raw),
                    None => match peg.as_ref() {
                        Some(_) => {
                            let prose = self.drop_name_collisions(
                                &self.currency_first(&rewrite(&source, st.prev.as_deref(), &vars)),
                                &vars,
                            );
                            if prose.trim().is_empty() || prose == expr {
                                None
                            } else {
                                eval(&prose)
                            }
                        }
                        None => None,
                    },
                }
            }
        };

        if result.is_some() {
            let assigned = peg
                .as_ref()
                .and_then(|program| program.name.clone().map(|name| (name, program.percent)))
                .or_else(|| {
                    ASSIGN
                        .captures(trimmed)
                        .map(|c| (c[1].to_string(), named_percent(c[2].trim())))
                });
            if let Some((name, percent)) = assigned {
                st.percents.retain(|(n, ..)| *n != name);
                if let Some(percent) = percent {
                    let re = Regex::new(&format!(r"([+-]\s*)\b{}\b", regex::escape(&name)));
                    if let Ok(re) = re {
                        st.percents.push((name.clone(), re, format!("{percent}%")));
                    }
                }
                st.vars.insert(name);
            }
        }

        let radix_out = peg
            .as_ref()
            .map_or_else(|| BASE_CONV.is_match(trimmed), |program| program.radix);
        let scientific = peg.as_ref().is_some_and(|program| program.scientific);
        let result = result.map(|raw| {
            if radix_out || scientific {
                raw
            } else {
                precisely(&mut st.ctx, &budget, &var, raw)
            }
        });

        match result {
            Some(raw) => {
                if !is_total {
                    st.block.push(Term {
                        var: var.clone(),
                        plain: is_plain_number(&raw),
                        aside: raw.ends_with('%') || raw.contains(" / "),
                    });
                }
                st.prev = Some(var);
                let display = if scientific {
                    crate::engine::peg::scientific(&raw)
                } else if radix_out {
                    raw.trim_start_matches("approx. ").to_string()
                } else if let Some(time) = transfer_time(trimmed, &raw) {
                    time
                } else {
                    pretty(&raw)
                };
                let display = match target {
                    Some((_, name)) => format!("{display} {name}"),
                    None => display,
                };

                let display = match pre_money_terms(trimmed) {
                    Some((raise, pre)) => {
                        let share = self.round_share(
                            &mut st.ctx,
                            &budget,
                            &raise,
                            &pre,
                            st.prev.as_deref(),
                            &st.vars,
                        );
                        match share {
                            Some(pct) => format!("{display} post, {pct}"),
                            None => format!("{display} post"),
                        }
                    }
                    None => display,
                };
                LineResult::value(display)
            }
            None => {
                let raw = budget.note();
                let noted = raw
                    .filter(|n| !n.starts_with("mod ") || MOD_WRITTEN.is_match(trimmed))
                    .or_else(|| crate::engine::time::note(trimmed));
                match noted {
                    Some(n) => LineResult::error(Some(n)),
                    None if raw.is_some() => LineResult::error(None),
                    None => match self.name_hint(trimmed, &st.vars, defs) {
                        Some(h) => LineResult::hint(h),
                        None => LineResult::error(None),
                    },
                }
            }
        }
    }

    fn money_times_unit(&self, raw: &str) -> bool {
        let tokens: Vec<&str> = raw.trim_start_matches("approx. ").split(' ').collect();
        let money = |t: &str| t.contains(['$', '£', '€', '¥', '₹']) || self.rates.knows(t);
        !tokens.contains(&"/")
            && tokens.iter().any(|t| money(t))
            && tokens
                .iter()
                .any(|t| t.chars().all(char::is_alphabetic) && !money(t))
    }

    fn units_as_prose(&self, expr: &str, vars: &HashSet<String>) -> String {
        let (lhs, rhs) = match ASSIGN.captures(expr) {
            Some(c) => (format!("{} = ", &c[1]), c[2].to_string()),
            None => (String::new(), expr.to_string()),
        };
        let rhs = LETTERS.replace_all(&rhs, |c: &regex::Captures| {
            let m = c.get(0).unwrap();
            let w = m.as_str();
            let before = &rhs[..m.start()];
            let glued = before.ends_with(|ch: char| ch.is_ascii_digit() || ch == '_');

            let keep = vars.contains(w)
                || self.rates.knows(w)
                || (glued && matches!(w, "k" | "K" | "M"))
                || before.ends_with('_')
                || stuck_to_money(before)
                || matches!(
                    w.to_ascii_lowercase().as_str(),
                    "in" | "to"
                        | "as"
                        | "of"
                        | "per"
                        | "mod"
                        | "pi"
                        | "e"
                        | "sqrt"
                        | "ln"
                        | "log"
                        | "abs"
                        | "round"
                        | "floor"
                        | "ceil"
                        | "sin"
                        | "cos"
                        | "tan"
                );
            if keep {
                w.to_string()
            } else {
                format!(" _w{w}")
            }
        });
        format!("{lhs}{rhs}")
    }

    fn round_share(
        &self,
        ctx: &mut fend_core::Context,
        budget: &Budget,
        raise: &str,
        pre: &str,
        prev: Option<&str>,
        vars: &HashSet<String>,
    ) -> Option<String> {
        let expr = format!("(({raise}) / (({raise}) + ({pre}))) in %");
        let expr =
            self.drop_name_collisions(&self.currency_first(&rewrite(&expr, prev, vars)), vars);
        let raw = budget.evaluate(&expr, ctx).ok()?;
        let shown = pretty(raw.get_main_result().trim());
        shown.ends_with('%').then_some(shown)
    }

    fn currency_first(&self, expr: &str) -> String {
        CODE_FIRST
            .replace_all(expr, |c: &regex::Captures| {
                if self.rates.knows(&c[1]) {
                    format!("{} {}", &c[2], &c[1])
                } else {
                    c[0].to_string()
                }
            })
            .into_owned()
    }

    fn drop_name_collisions(&self, expr: &str, vars: &HashSet<String>) -> String {
        let assigned = ASSIGN.captures(expr).map(|c| c[1].to_string());
        WORD.replace_all(expr, |c: &regex::Captures| {
            let w = &c[0];
            if vars.contains(w) || assigned.as_deref() == Some(w) {
                return w.to_string();
            }
            let m = c.get(0).unwrap();
            let title = w.chars().next().is_some_and(char::is_uppercase)
                && w.chars().skip(1).all(char::is_lowercase);
            let before = expr[..m.start()].trim_end();
            let after = expr[m.end()..].trim_start();
            let alone = before.is_empty() && after.is_empty();
            let in_expr = before.ends_with(|ch: char| "+-*/^=(,".contains(ch))
                || after.starts_with(|ch: char| "+-*/^=),".contains(ch));
            if alone || in_expr {
                return w.to_string();
            }
            let after_amount = before.ends_with(|ch: char| ch.is_ascii_digit() || ch == ')');
            let after_conversion = before
                .rsplit(|ch: char| !ch.is_alphabetic())
                .next()
                .is_some_and(|word| {
                    matches!(
                        word.to_ascii_lowercase().as_str(),
                        "in" | "to" | "as" | "per"
                    )
                });
            let upper = w.to_ascii_uppercase();
            if title && !after_amount && !after_conversion && self.rates.knows(&upper) {
                String::new()
            } else {
                w.to_string()
            }
        })
        .into_owned()
    }

    fn fend_knows_word(&self, word: &str) -> bool {
        let mut ctx = self.context();
        let budget = Budget::new(None, LINE_MS);
        budget.evaluate(word, &mut ctx).is_ok()
    }

    fn name_hint(
        &self,
        line: &str,
        above: &HashSet<String>,
        defs: &HashSet<String>,
    ) -> Option<String> {
        let code = line.split("//").next().unwrap_or(line);
        let lhs = ASSIGN
            .captures(code)
            .and_then(|c| c.get(1))
            .map(|m| m.range());
        let mut meant = None;
        let mut below = None;
        let mut unknown = None;

        let mut loose = false;
        for c in IDENT.captures_iter(code) {
            let m = c.get(0).unwrap();
            let w = m.as_str();
            if lhs.as_ref().is_some_and(|r| r.contains(&m.start())) {
                continue;
            }
            if above.contains(w) {
                continue;
            }
            if unit_position(code, m.start()) || call_position(code, m.end()) {
                continue;
            }
            if let Some(name) = case_suggestion(w, above, defs) {
                meant.get_or_insert_with(|| format!("Did you mean {name}?"));
                continue;
            }
            if defs.contains(w) {
                below.get_or_insert_with(|| format!("{w} is defined below"));
                continue;
            }
            if is_name_keyword(w) || self.bare_currency(w) {
                continue;
            }

            if code.trim() == w || expr_position(code, m.start(), m.end()) {
                unknown.get_or_insert_with(|| format!("Unknown name '{w}'"));
            } else {
                loose = true;
            }
        }
        if let Some(s) = meant {
            return Some(s);
        }
        if let Some(s) = below {
            return Some(s);
        }
        if unknown.is_some() && !loose && mentions_math(code) {
            return unknown;
        }
        None
    }

    fn bare_currency(&self, w: &str) -> bool {
        w.len() == 3 && w.bytes().all(|b| b.is_ascii_uppercase()) && self.rates.knows(w)
    }

    fn with_currency(
        &self,
        ctx: &mut fend_core::Context,
        budget: &Budget,
        var: &str,
        expr: &str,
    ) -> Option<String> {
        let unit = currency_vs_plain(budget)?;
        let (lhs, rhs) = match ASSIGN.captures(expr) {
            Some(c) => (Some(c[1].to_string()), c[2].to_string()),
            None => (None, expr.to_string()),
        };
        let mut rhs = plain_terms_in(&rhs, &unit);
        loop {
            let next = BARE_NUMBER
                .replace_all(&rhs, |c: &regex::Captures| {
                    let amount = if unit.chars().all(|ch| ch.is_ascii_alphabetic()) {
                        format!("({} {unit})", &c[2])
                    } else {
                        format!("({unit}{})", &c[2])
                    };
                    format!("{}{amount}{}", &c[1], &c[3])
                })
                .into_owned();
            if next == rhs {
                break;
            }
            rhs = next;
        }
        let expr = match lhs {
            Some(name) => format!("{name} = {rhs}"),
            None => rhs,
        };
        eval_into(ctx, budget, var, &expr)
    }

    pub fn hover_at(&self, text: &str, cursor: usize) -> Option<Hover> {
        let (line_idx, line_at, token) = token_at(text, cursor)?;
        let lines: Vec<&str> = text.split('\n').collect();
        let line = *lines.get(line_idx)?;
        if token_skipped(line, &token) {
            return None;
        }
        let mut st = self.fresh();
        let defs = defined_variables(text);
        for (i, prev) in lines.iter().enumerate().take(line_idx) {
            self.eval_in(&mut st, i, prev, &defs);
        }
        let value = if token_is_definition(line, &token) {
            shown_value(&self.eval_in(&mut st, line_idx, line, &defs))?
        } else if token.number {
            let value = shown_value(&self.eval_math(&mut st, line_idx, &token.text, &defs))?;
            if value == token.text {
                return None;
            }
            value
        } else if st.vars.contains(&token.text) {
            shown_value(&self.eval_math(&mut st, line_idx, &token.text, &defs))?
        } else {
            return None;
        };
        let used_by = if token_is_definition(line, &token) {
            uses_elsewhere(text, line_idx, &token.text)
        } else {
            0
        };
        Some(Hover {
            start: line_at + token.start,
            end: line_at + token.end,
            value,
            used_by,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hover {
    pub start: usize,
    pub end: usize,
    pub value: String,
    pub used_by: usize,
}

pub fn realign_map(old: &str, new: &str) -> Vec<(Option<usize>, bool)> {
    let (a, b): (Vec<&str>, Vec<&str>) = (old.split('\n').collect(), new.split('\n').collect());
    let top = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let bottom = a[top..]
        .iter()
        .rev()
        .zip(b[top..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let same = a.len() == b.len() && top == a.len();
    (0..b.len())
        .map(|i| {
            if i < top {
                (Some(i), false)
            } else if i >= b.len() - bottom {
                (Some(i + a.len() - b.len()), !same)
            } else {
                (None, false)
            }
        })
        .collect()
}

pub fn realign(old: &str, results: &[LineResult], new: &str) -> Vec<LineResult> {
    realign_onto(old, results, new, &[])
}

pub fn realign_onto(
    old: &str,
    results: &[LineResult],
    new: &str,
    shown: &[LineResult],
) -> Vec<LineResult> {
    realign_map(old, new)
        .into_iter()
        .enumerate()
        .map(|(i, (from, below))| {
            let (r, stale) = match from.and_then(|j| results.get(j)) {
                Some(r) => (r, below),
                None => match shown.get(i) {
                    Some(r) => (r, true),
                    None => return LineResult::new(Kind::Empty),
                },
            };
            LineResult {
                stale: r.stale || stale,
                ..r.clone()
            }
        })
        .collect()
}

pub fn with_answers(text: &str, results: &[LineResult]) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let answered = |i: usize| results.get(i).is_some_and(|r| !r.display.is_empty());
    let width = lines
        .iter()
        .enumerate()
        .filter(|(i, _)| answered(*i))
        .map(|(_, l)| l.trim_end().chars().count())
        .max()
        .unwrap_or(0)
        .min(56)
        + 4;
    let mut out: Vec<String> = lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let line = line.trim_end();
            match results.get(i).filter(|_| answered(i)) {
                Some(r) => format!("{line:<width$}{}", r.display),
                None => line.to_string(),
            }
        })
        .collect();
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out.join("\n")
}

pub fn defined_variables(text: &str) -> std::collections::HashSet<String> {
    text.lines()
        .filter_map(|l| {
            ASSIGN
                .captures(markdown::math_body(l))
                .map(|c| c[1].to_string())
        })
        .collect()
}

pub fn definition_span(line: &str) -> Option<(usize, usize)> {
    let class = markdown::classify(line);
    let body = match class.form {
        markdown::Form::Blank | markdown::Form::Comment | markdown::Form::Table => return None,
        _ => class.body,
    };
    let name = ASSIGN.captures(body)?.get(1)?;
    let base = subslice_offset(line, body)?;
    Some((base + name.start(), base + name.end()))
}

fn subslice_offset(whole: &str, part: &str) -> Option<usize> {
    let start = part.as_ptr() as usize;
    let base = whole.as_ptr() as usize;
    let offset = start.checked_sub(base)?;
    (offset + part.len() <= whole.len()).then_some(offset)
}

fn shown_value(result: &LineResult) -> Option<String> {
    (result.kind == Kind::Value && !result.display.is_empty()).then(|| result.display.clone())
}

#[derive(Clone)]
struct Tok {
    start: usize,
    end: usize,
    text: String,
    number: bool,
}

fn uses_elsewhere(text: &str, line_idx: usize, name: &str) -> usize {
    text.split('\n')
        .enumerate()
        .filter(|(i, _)| *i != line_idx)
        .filter(|(_, line)| {
            !matches!(
                markdown::classify(line).form,
                markdown::Form::Comment | markdown::Form::Blank | markdown::Form::Heading(_)
            )
        })
        .filter(|(_, line)| mentions(line, name))
        .count()
}

fn mentions(line: &str, name: &str) -> bool {
    let mut start = 0;
    let mut inside = false;
    for (i, c) in line.char_indices() {
        let ident = c.is_ascii_alphanumeric() || c == '_';
        if ident {
            if !inside {
                start = i;
            }
        } else if inside && &line[start..i] == name {
            return true;
        }
        inside = ident;
    }
    inside && &line[start..] == name
}

fn token_at(text: &str, cursor: usize) -> Option<(usize, usize, Tok)> {
    if cursor > text.chars().count() {
        return None;
    }
    let mut line_idx = 0;
    let mut line_at = 0;
    for (i, ch) in text.chars().enumerate() {
        if i == cursor {
            break;
        }
        if ch == '\n' {
            line_idx += 1;
            line_at = i + 1;
        }
    }
    let line = text.split('\n').nth(line_idx)?;
    let token = token_in_line(line, cursor - line_at)?;
    Some((line_idx, line_at, token))
}

fn token_in_line(line: &str, col: usize) -> Option<Tok> {
    let tokens = scan_tokens(line);
    if let Some(tok) = tokens.iter().find(|t| col >= t.start && col < t.end) {
        return Some(tok.clone());
    }
    tokens
        .into_iter()
        .rev()
        .find(|t| t.end == col && col > t.start)
}

fn scan_tokens(line: &str) -> Vec<Tok> {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        if is_currency(c) {
            if chars
                .get(i + 1)
                .is_some_and(|n| n.is_ascii_digit() || *n == '.')
            {
                let start = i;
                i = take_suffix(&chars, take_number(&chars, i + 1));
                out.push(tok(&chars, start, i, true));
            } else {
                i += 1;
            }
        } else if c.is_ascii_digit() {
            let start = i;
            i = take_suffix(&chars, take_number(&chars, i));
            out.push(tok(&chars, start, i, true));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(tok(&chars, start, i, false));
        } else {
            i += 1;
        }
    }
    out
}

fn tok(chars: &[char], start: usize, end: usize, number: bool) -> Tok {
    Tok {
        start,
        end,
        text: chars[start..end].iter().collect(),
        number,
    }
}

fn take_number(chars: &[char], mut i: usize) -> usize {
    while i < chars.len()
        && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == ',' || chars[i] == '_')
    {
        i += 1;
    }
    i
}

fn take_suffix(chars: &[char], i: usize) -> usize {
    if chars.get(i) == Some(&'%') {
        return i + 1;
    }
    let mut j = i;
    while j < chars.len() && chars[j].is_ascii_alphabetic() && j - i < 2 {
        j += 1;
    }
    let word: String = chars[i..j].iter().collect::<String>().to_ascii_lowercase();
    if matches!(word.as_str(), "k" | "m" | "bn" | "b") {
        j
    } else {
        i
    }
}

fn is_currency(c: char) -> bool {
    "$£€¥₹₩₽₺".contains(c)
}

fn token_skipped(line: &str, token: &Tok) -> bool {
    let from = char_to_byte(line, token.start);
    let class = markdown::classify(line);
    if class.comment_at.is_some_and(|at| from >= at) {
        return true;
    }
    if matches!(
        class.form,
        markdown::Form::Comment | markdown::Form::Table | markdown::Form::Blank
    ) {
        return true;
    }
    let mut quoted = false;
    for (i, c) in line.char_indices() {
        if i >= from {
            break;
        }
        if c == '"' {
            quoted = !quoted;
        }
    }
    quoted
}

fn token_is_definition(line: &str, token: &Tok) -> bool {
    let Some((from, to)) = definition_span(line) else {
        return false;
    };
    (
        char_to_byte(line, token.start),
        char_to_byte(line, token.end),
    ) == (from, to)
}

fn char_to_byte(line: &str, chars: usize) -> usize {
    line.chars().take(chars).map(char::len_utf8).sum()
}

fn named_percent(value: &str) -> Option<f64> {
    let body = value.trim().strip_suffix('%')?.trim();
    body.parse::<f64>().ok()
}

pub(crate) fn aggregate(line: &str) -> Option<bool> {
    let lower = line.to_ascii_lowercase();
    match lower.trim_end_matches(':').trim() {
        "sum" | "total" => Some(false),
        "avg" | "average" | "mean" => Some(true),
        _ => None,
    }
}

fn total_into(
    ctx: &mut fend_core::Context,
    budget: &Budget,
    var: &str,
    block: &[Term],
    avg: bool,
    name: Option<&str>,
) -> Option<String> {
    let amounts: Vec<Term> = block.iter().filter(|t| !t.aside).cloned().collect();
    let mut tries = vec![block];
    if amounts.len() < block.len() {
        tries.push(&amounts);
    }
    tries
        .into_iter()
        .find_map(|block| sum_into(ctx, budget, var, block, avg, name))
}

fn sum_into(
    ctx: &mut fend_core::Context,
    budget: &Budget,
    var: &str,
    block: &[Term],
    avg: bool,
    name: Option<&str>,
) -> Option<String> {
    let expr = |unit: Option<&str>| {
        let terms: Vec<String> = block
            .iter()
            .map(|t| match unit {
                Some(u) if t.plain && u.chars().all(|ch| ch.is_ascii_alphabetic()) => {
                    format!("({} * 1 {u})", t.var)
                }
                Some(u) if t.plain => format!("({} * ({u}1))", t.var),
                _ => t.var.clone(),
            })
            .collect();
        let sum = match (terms.is_empty(), avg) {
            (true, _) => "0".to_string(),
            (false, false) => terms.join(" + "),
            (false, true) => format!("({}) / {}", terms.join(" + "), terms.len()),
        };
        match name {
            Some(name) => format!("{name} = {sum}"),
            None => sum,
        }
    };
    eval_into(ctx, budget, var, &expr(None)).or_else(|| {
        let unit = currency_vs_plain(budget)?;
        eval_into(ctx, budget, var, &expr(Some(&unit)))
    })
}

fn totals_inside(expr: &str, vars: &HashSet<String>, i: usize) -> (String, [Option<String>; 2]) {
    let mut used = [None, None];
    let out = TOTAL_WORD
        .replace_all(expr, |c: &regex::Captures| {
            let m = c.get(0).unwrap();
            let before = expr[..m.start()].trim_end();
            let after = expr[m.end()..].trim_start();
            let ok_before =
                before.is_empty() || before.ends_with(|ch: char| "+-*/×÷^(=,".contains(ch));
            let ok_after = after.is_empty()
                || after.starts_with(|ch: char| "+-*/×÷^)".contains(ch))
                || CONVERSION_START.is_match(after);
            if vars.contains(m.as_str()) || !ok_before || !ok_after {
                return m.as_str().to_string();
            }
            let avg = !matches!(m.as_str().to_ascii_lowercase().as_str(), "total" | "sum");
            let name = format!("{}{i}", if avg { "_a" } else { "_t" });
            used[usize::from(avg)] = Some(name.clone());
            name
        })
        .into_owned();
    (out, used)
}

fn plain_terms_in(expr: &str, unit: &str) -> String {
    let mut terms = Vec::new();
    let (mut depth, mut start, mut last) = (0i32, 0, ' ');
    for (at, ch) in expr.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            '+' | '-' if depth == 0 && (last.is_ascii_digit() || last == ')') => {
                terms.push(&expr[start..at]);
                terms.push(&expr[at..at + 1]);
                start = at + 1;
            }
            _ => {}
        }
        if !ch.is_whitespace() {
            last = ch;
        }
    }
    terms.push(&expr[start..]);
    terms
        .into_iter()
        .map(|t| {
            let arithmetic = t.chars().any(|ch| "*/^".contains(ch))
                && t.chars()
                    .all(|ch| ch.is_ascii_digit() || " .,()*/^".contains(ch));
            match arithmetic {
                false => t.to_string(),
                true if unit.chars().all(|ch| ch.is_ascii_alphabetic()) => {
                    format!(" (({}) * 1 {unit}) ", t.trim())
                }
                true => format!(" (({}) * ({unit}1)) ", t.trim()),
            }
        })
        .collect::<String>()
}

fn currency_vs_plain(budget: &Budget) -> Option<String> {
    let error = budget.error.lock().unwrap().clone()?;
    let c = CURRENCY_VS_PLAIN.captures(&error)?;
    match (&c[3], &c[4]) {
        ("unitless", "BASE_CURRENCY") => Some(c[2].to_string()),
        ("BASE_CURRENCY", "unitless") => Some(c[1].to_string()),
        _ => None,
    }
}

fn is_plain_number(raw: &str) -> bool {
    raw.trim_start_matches("approx. ")
        .replace(',', "")
        .parse::<f64>()
        .is_ok()
}

fn canonical_period(word: &str) -> &'static str {
    match word.to_ascii_lowercase().as_str() {
        "year" | "years" | "yr" | "annually" | "arr" => "year",
        "month" | "months" | "mo" | "monthly" | "mrr" => "month",
        "week" | "weeks" => "week",
        "day" | "days" => "day",
        _ => "year",
    }
}

fn money_per(expr: &str, period: &str) -> String {
    let period = canonical_period(period);
    if expr.contains('£') || expr.contains("GBP") {
        format!("GBP/{period}")
    } else if expr.contains('€') || expr.contains("EUR") {
        format!("EUR/{period}")
    } else if expr.contains('₹') || expr.contains("INR") {
        format!("INR/{period}")
    } else if expr.contains('¥') || expr.contains("JPY") {
        format!("JPY/{period}")
    } else if expr.contains('$') || expr.contains("USD") {
        format!("$/{period}")
    } else {
        format!("1/{period}")
    }
}

fn expand_money_scales(s: &str) -> String {
    MONEY_SCALE
        .replace_all(s, |c: &regex::Captures| {
            let places = match c[3].to_ascii_lowercase().as_str() {
                "k" => 3,
                "m" | "mm" => 6,
                "b" | "bn" => 9,
                _ => return c[0].to_string(),
            };
            format!("{}{}", &c[1], shift_point(&c[2].replace(',', ""), places))
        })
        .into_owned()
}

fn expand_scales(s: &str) -> String {
    SCALE_SUFFIX
        .replace_all(s, |c: &regex::Captures| {
            let places = if &c[3] == "M" { 6 } else { 3 };
            format!("{}{}", &c[1], shift_point(&c[2].replace(',', ""), places))
        })
        .into_owned()
}

fn group_rate_divisors(s: &str) -> String {
    RATE_DIVISOR
        .replace_all(s, |c: &regex::Captures| {
            format!("/ ({} / {})", c[1].trim(), c[2].to_ascii_lowercase())
        })
        .into_owned()
}

fn rescale_rates(s: &str) -> String {
    RATE_IN_TIME
        .replace_all(s, |c: &regex::Captures| {
            let at = c.get(0).unwrap().start();
            if s[..at].contains('/') {
                return c[0].to_string();
            }
            format!("{} in {}", &c[1], money_per(s, &c[2]))
        })
        .into_owned()
}

fn period_rate(word: &str) -> &'static str {
    match word.to_ascii_lowercase().as_str() {
        "annually" | "arr" => "per year",
        _ => "per month",
    }
}

fn valuation(rhs: &str) -> Option<String> {
    let rhs = rhs.trim();
    if let Some(c) = AT_PRE.captures(rhs) {
        return Some(format!("({}) + ({})", c[1].trim(), c[2].trim()));
    }
    if let Some(c) = AT_POST.captures(rhs) {
        return Some(format!("(({}) / ({})) in %", c[1].trim(), c[2].trim()));
    }
    if let Some(c) = POST_AT.captures(rhs) {
        return Some(format!("(({}) / ({})) in %", c[1].trim(), c[2].trim()));
    }
    None
}

fn calc_rhs(line: &str) -> String {
    let line = match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    };
    let line = line.trim();
    let owned;
    let line = match LABEL.captures(line) {
        Some(c) => {
            owned = c[2].trim().to_string();
            owned.as_str()
        }
        None => line,
    };
    match ASSIGN.captures(line) {
        Some(c) => c[2].trim().to_string(),
        None => line.to_string(),
    }
}

fn pre_money_terms(line: &str) -> Option<(String, String)> {
    let rhs = calc_rhs(line);
    let c = AT_PRE.captures(&rhs)?;
    Some((c[1].trim().to_string(), c[2].trim().to_string()))
}

fn map_rhs(s: &str, f: impl FnOnce(&str) -> String) -> String {
    match ASSIGN.captures(s) {
        Some(c) => format!("{} = {}", &c[1], f(c[2].trim())),
        None => f(s),
    }
}

fn founder_line(s: &str, vars: &HashSet<String>) -> String {
    map_rhs(s, |rhs| {
        if let Some(done) = valuation(rhs) {
            return done;
        }
        let kept = |word: &str| vars.contains(word);
        let s = PERIOD_OF_RATE
            .replace_all(rhs, |c: &regex::Captures| {
                let into = if c[2].eq_ignore_ascii_case("monthly") {
                    "months"
                } else {
                    "years"
                };
                format!("{} in {into}", &c[1])
            })
            .into_owned();
        let s = PERIOD_TARGET
            .replace_all(&s, |c: &regex::Captures| {
                if kept(&c[2]) {
                    c[0].to_string()
                } else {
                    format!("{} {}", &c[1], canonical_period(&c[2]))
                }
            })
            .into_owned();
        PERIOD_WORD
            .replace_all(&s, |c: &regex::Captures| {
                if kept(&c[2]) {
                    c[0].to_string()
                } else {
                    format!("{} {}", &c[1], period_rate(&c[2]))
                }
            })
            .into_owned()
    })
}

fn expand_calls(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if let Some((name, paren)) = match_call(&chars, i) {
            if let Some(end) = matching_paren(&chars, paren) {
                let inside: String = chars[paren + 1..end].iter().collect();
                let args: Vec<String> = split_args(&inside)
                    .into_iter()
                    .filter(|a| !a.is_empty())
                    .map(|a| expand_calls(&a))
                    .collect();
                match render_call(name, &args) {
                    Some(rendered) => out.push_str(&rendered),
                    None => out.extend(chars[i..=end].iter()),
                }
                i = end + 1;
                continue;
            }

            out.extend(chars[i..].iter());
            break;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

const CALLS: &[&str] = &["average", "total", "mean", "sum", "avg", "min", "max"];

fn match_call(chars: &[char], i: usize) -> Option<(&'static str, usize)> {
    if i > 0 && is_ident_char(chars[i - 1]) {
        return None;
    }
    for name in CALLS {
        let n: Vec<char> = name.chars().collect();
        if i + n.len() > chars.len() {
            continue;
        }
        if !chars[i..i + n.len()]
            .iter()
            .zip(&n)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
        {
            continue;
        }
        let after_name = i + n.len();
        if chars.get(after_name).is_some_and(|c| is_ident_char(*c)) {
            continue;
        }
        let mut p = after_name;
        while chars.get(p).is_some_and(|c| c.is_whitespace()) {
            p += 1;
        }
        if chars.get(p) == Some(&'(') {
            return Some((*name, p));
        }
    }
    None
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn matching_paren(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (i, c) in chars.iter().enumerate().skip(open) {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_args(inside: &str) -> Vec<String> {
    let chars: Vec<char> = inside.chars().collect();
    let mut args = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    for (i, c) in chars.iter().enumerate() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 && !is_thousands_comma(&chars, i) => {
                args.push(
                    chars[start..i]
                        .iter()
                        .collect::<String>()
                        .trim()
                        .to_string(),
                );
                start = i + 1;
            }
            _ => {}
        }
    }
    let last = chars[start..].iter().collect::<String>().trim().to_string();
    if args.is_empty() && last.is_empty() {
        return Vec::new();
    }
    args.push(last);
    args
}

fn is_thousands_comma(chars: &[char], i: usize) -> bool {
    if chars.get(i + 1).is_some_and(|c| c.is_whitespace()) {
        return false;
    }
    let mut right = 0;
    let mut r = i + 1;
    while chars.get(r).is_some_and(|c| c.is_ascii_digit()) {
        right += 1;
        r += 1;
        if right > 3 {
            return false;
        }
    }
    if right != 3 {
        return false;
    }
    let mut left = 0;
    let mut l = i;
    while l > 0 && chars[l - 1].is_ascii_digit() {
        left += 1;
        l -= 1;
        if left > 3 {
            return false;
        }
    }
    if left == 0 || left > 3 {
        return false;
    }
    if l > 0 && chars[l - 1] == ',' && left != 3 {
        return false;
    }
    true
}

fn render_call(name: &str, args: &[String]) -> Option<String> {
    let wrapped: Vec<String> = args.iter().map(|a| format!("({a})")).collect();
    match name.to_ascii_lowercase().as_str() {
        "sum" | "total" => {
            if wrapped.is_empty() {
                Some("0".to_string())
            } else {
                Some(format!("({})", wrapped.join("+")))
            }
        }
        "avg" | "average" | "mean" => {
            let n = wrapped.len();
            (n > 0).then(|| format!("(({})/{n})", wrapped.join("+")))
        }
        "max" => (!wrapped.is_empty()).then(|| fold_extreme(true, &wrapped)),
        "min" => (!wrapped.is_empty()).then(|| fold_extreme(false, &wrapped)),
        _ => None,
    }
}

fn fold_extreme(max: bool, args: &[String]) -> String {
    let mut acc = args[0].clone();
    let sign = if max { "+" } else { "-" };
    for arg in &args[1..] {
        acc = format!("(({acc}+{arg}{sign}abs({acc}-{arg}))/2)");
    }
    acc
}

fn unit_position(code: &str, start: usize) -> bool {
    let before = code[..start].trim_end();
    if before.ends_with(|ch: char| ch.is_ascii_digit() || ch == ')') {
        return true;
    }
    let prev = before
        .rsplit(|ch: char| !ch.is_alphabetic())
        .next()
        .unwrap_or("");
    matches!(
        prev.to_ascii_lowercase().as_str(),
        "in" | "to" | "as" | "per"
    )
}

fn call_position(code: &str, end: usize) -> bool {
    code[end..].trim_start().starts_with('(')
}

fn expr_position(code: &str, start: usize, end: usize) -> bool {
    let before = code[..start].trim_end();
    let after = code[end..].trim_start();
    before.ends_with(|ch: char| "+-*/^=(,".contains(ch))
        || after.starts_with(|ch: char| "+-*/^=),".contains(ch))
}

fn case_suggestion(word: &str, above: &HashSet<String>, defs: &HashSet<String>) -> Option<String> {
    let pick = |set: &HashSet<String>| {
        let mut hits: Vec<&String> = set
            .iter()
            .filter(|n| n.eq_ignore_ascii_case(word) && n.as_str() != word)
            .collect();
        hits.sort();
        (hits.len() == 1).then(|| hits[0].clone())
    };
    pick(above).or_else(|| pick(defs))
}

fn mentions_math(code: &str) -> bool {
    let t = code.trim();
    if t.contains(['=', '+', '-', '*', '/', '^', '%', '(']) {
        return true;
    }
    let starts_name = t
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    !t.is_empty()
        && !t.contains(char::is_whitespace)
        && starts_name
        && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_name_keyword(w: &str) -> bool {
    matches!(
        w.to_ascii_lowercase().as_str(),
        "in" | "to"
            | "as"
            | "of"
            | "per"
            | "mod"
            | "on"
            | "off"
            | "is"
            | "and"
            | "or"
            | "the"
            | "a"
            | "an"
            | "at"
            | "for"
            | "with"
            | "by"
            | "from"
            | "plus"
            | "minus"
            | "times"
            | "prev"
            | "sum"
            | "total"
            | "avg"
            | "average"
            | "mean"
            | "min"
            | "max"
            | "today"
            | "tomorrow"
            | "yesterday"
            | "now"
            | "sqrt"
            | "ln"
            | "log"
            | "abs"
            | "round"
            | "floor"
            | "ceil"
            | "sin"
            | "cos"
            | "tan"
            | "pi"
            | "e"
            | "tau"
            | "phi"
            | "hex"
            | "hexadecimal"
            | "binary"
            | "bin"
            | "octal"
            | "not"
    )
}

pub fn rewrite(line: &str, prev: Option<&str>, vars: &HashSet<String>) -> String {
    let mut s = match line.find("//") {
        Some(idx) => line[..idx].to_string(),
        None => line.to_string(),
    };
    s = QUOTED.replace_all(&s, " ").into_owned();

    s = expand_calls(&s);

    for _ in 0..2 {
        s = DOT_THOUSANDS
            .replace_all(&s, |c: &regex::Captures| {
                format!("{}{}.{}{}", &c[1], c[2].replace('.', ""), &c[3], &c[4])
            })
            .into_owned();
        s = DOT_MILLIONS
            .replace_all(&s, |c: &regex::Captures| {
                format!("{}{}{}", &c[1], c[2].replace('.', ""), &c[3])
            })
            .into_owned();
        s = DECIMAL_COMMA
            .replace_all(&s, "${1}${2}.${3}${4}")
            .into_owned();
    }
    s = SYMBOL_MINUS.replace_all(&s, "$1-$2$3").into_owned();

    s = expand_money_scales(&s);
    s = PREFIXED_DOLLAR
        .replace_all(&s, |c: &regex::Captures| {
            let code = match &c[1] {
                "US" => "USD",
                "CA" | "C" => "CAD",
                "AU" | "A" => "AUD",
                "NZ" => "NZD",
                "HK" => "HKD",
                _ => "SGD",
            };
            format!("{} {code}", &c[2])
        })
        .into_owned();
    s = VERSION_WORD.replace_all(&s, " ").into_owned();
    s = PERCENT_WORD.replace_all(&s, "$1%").into_owned();
    s = PCT_GLUED.replace_all(&s, "% $1").into_owned();
    s = INCHES.replace_all(&s, "$1 inches$2").into_owned();
    s = LAKH_CRORE
        .replace_all(&s, |c: &regex::Captures| {
            let zeros = if c[2].to_ascii_lowercase().starts_with('c') {
                7
            } else {
                5
            };
            shift_point(&c[1].replace(',', ""), zeros)
        })
        .into_owned();
    s = HALF.replace_all(&s, "0.5 * ").into_owned();
    s = THIRD.replace_all(&s, "(1/3) * ").into_owned();
    s = QUARTER.replace_all(&s, "0.25 * ").into_owned();
    s = TWICE.replace_all(&s, "2 * ").into_owned();
    s = TRIPLE.replace_all(&s, "3 * ").into_owned();

    if let Some(c) = LABEL.captures(&s) {
        s = c[2].to_string();
    }

    if let Some(c) = SPLIT.captures(&s) {
        let amount = c.get(1).or(c.get(2)).map_or("", |m| m.as_str());
        let ways = c.get(3).or(c.get(4)).map_or("", |m| m.as_str());
        s = format!("({amount}) / {ways}");
    }
    if let Some(c) = TRANSFER.captures(&s) {
        s = format!("({}) / ({})", &c[1], &c[2]);
    }

    s = group_rate_divisors(&s);
    s = expand_scales(&s);

    s = SYMBOL_NUM
        .replace_all(&s, |c: &regex::Captures| {
            let sym = c[1].chars().next().unwrap();
            format!("{} {}", &c[2], symbol_code(sym))
        })
        .into_owned();
    for (sym, code) in SYMBOL_CODES {
        s = s.replace(*sym, code);
    }

    s = MONEY_PREFIX.replace_all(&s, "($0)").into_owned();

    if let Some(c) = LABEL_NUMBER.captures(&s) {
        s = c[1].to_string();
    }

    if let Some(p) = prev {
        s = PREV.replace_all(&s, p).into_owned();
    }

    loop {
        let next = TIMES_X
            .replace_all(&s, |c: &regex::Captures| {
                if &c[1] == "0" && c[2].is_empty() && &c[3] == "x" {
                    c[0].to_string()
                } else {
                    format!("{} * {}", &c[1], &c[5])
                }
            })
            .into_owned();
        if next == s {
            break;
        }
        s = next;
    }
    s = founder_line(&s, vars);
    s = A_DAY.replace_all(&s, "$1 / $2").into_owned();
    s = AT_PRICE.replace_all(&s, "$1 * $2").into_owned();
    s = drop_prose_words(&s, vars);
    s = IMPERIAL_UNIT
        .replace_all(&s, |c: &regex::Captures| format!("({})", imperial(&c[1]).1))
        .into_owned();
    for (re, unit) in UNIT_ALIAS_RES.iter() {
        s = re.replace_all(&s, *unit).into_owned();
    }
    s = DIVISOR
        .replace_all(&s, |c: &regex::Captures| {
            let unit = c[3].split('/').next().unwrap_or("").to_ascii_lowercase();
            if KEYWORDS.contains(&unit.as_str()) {
                c[0].to_string()
            } else {
                format!("{}/ ({}{})", &c[1], &c[2], &c[3])
            }
        })
        .into_owned();
    s = rescale_rates(&s);

    let (lhs, mut rhs) = match ASSIGN.captures(&s) {
        Some(c) => (Some(c[1].to_string()), c[2].to_string()),
        None => (None, s.clone()),
    };

    for (re, op) in WORD_OP_RES.iter() {
        rhs = re.replace_all(&rhs, *op).into_owned();
    }

    rhs = PLUS_MINUS_PCT_OF
        .replace_all(&rhs, |c: &regex::Captures| {
            format!("{}(({} / 100) * {})", &c[1], c[2].replace(',', ""), &c[3])
        })
        .into_owned();
    rhs = PCT_SHARE.replace_all(&rhs, "($1 / 100)$2").into_owned();
    rhs = if let Some(c) = IS_WHAT_PCT.captures(&rhs) {
        format!("(({}) / ({})) to %", &c[1], &c[2])
    } else if let Some(c) = IS_PCT_OF_WHAT.captures(&rhs) {
        format!("({}) / (({}) / 100)", &c[1], &c[2])
    } else if let Some(c) = PCT_OF_WHAT.captures(&rhs) {
        format!("({}) / (({}) / 100)", &c[2], &c[1])
    } else if let Some(c) = PCT_ON.captures(&rhs) {
        format!("({}) * (1 + ({}) / 100)", &c[2], &c[1])
    } else if PCT_OFF.is_match(&rhs) {
        discounts(&rhs)
    } else if let Some(c) = AS_PCT.captures(&rhs) {
        format!("(({}) / ({})) to %", &c[1], &c[2])
    } else if PLUS_MINUS_PCT_STEP.is_match(&rhs) {
        let mut out = rhs;
        while let Some(c) = PLUS_MINUS_PCT_STEP.captures(&out) {
            let term = c[3].replace(',', "").replace('%', " / 100");
            let step = format!("(({}) * (1 {} ({term})))", &c[1], &c[2]);
            out = format!("{step}{}", &out[c.get(0).unwrap().end()..]);
        }
        out
    } else {
        rhs
    };

    rhs = TIMES_PCT.replace_all(&rhs, "$1($2 / 100)").into_owned();

    match lhs {
        Some(name) => format!("{name} = {rhs}"),
        None => rhs,
    }
}

fn discounts(s: &str) -> String {
    match PCT_OFF.captures(s) {
        Some(c) => format!("({}) * (1 - ({}) / 100)", discounts(&c[2]), &c[1]),
        None => s.to_string(),
    }
}

const KEYWORDS: &[&str] = &[
    "in", "to", "as", "of", "per", "mod", "on", "off", "is", "and",
];

fn drop_prose_words(s: &str, vars: &HashSet<String>) -> String {
    const PROSE: &[&str] = &[
        "I", "a", "A", "an", "An", "the", "The", "at", "At", "my", "My",
    ];

    const JOINS: &[&str] = &[
        "times",
        "plus",
        "minus",
        "multiplied",
        "divided",
        "by",
        "mod",
        "in",
        "to",
        "as",
        "of",
        "per",
        "x",
    ];
    let joins = |t: &str| {
        t.starts_with(['+', '-', '*', '/', '^', '=', '(', ')', '%', '×', '!'])
            || t.ends_with(['+', '-', '*', '/', '^', '=', '(', '×'])
            || JOINS.contains(&t.to_ascii_lowercase().as_str())
    };
    let assigned = ASSIGN.captures(s).map(|c| c[1].to_string());
    let toks: Vec<&str> = s.split(' ').collect();
    let mut out: Vec<&str> = Vec::new();
    for (i, &tok) in toks.iter().enumerate() {
        let prose = PROSE.contains(&tok);
        let var = vars.contains(tok) || assigned.as_deref() == Some(tok);

        let before = out.iter().rev().find(|t| !t.is_empty()).copied();
        let after = toks[i + 1..].iter().find(|t| !t.is_empty()).copied();
        let used = !prose
            || assigned.as_deref() == Some(tok)
            || (before.is_none() && after.is_none())
            || before.is_some_and(joins)
            || after.is_some_and(joins);
        if var && used {
            out.push(tok);
            continue;
        }
        let after_number =
            before.is_some_and(|t| t.ends_with(|c: char| c.is_ascii_digit() || c == ')'));
        if prose && !after_number {
            continue;
        }
        out.push(tok);
    }
    out.join(" ")
}

fn symbol_code(sym: char) -> &'static str {
    SYMBOL_CODES
        .iter()
        .find(|(c, _)| *c == sym)
        .map_or("", |(_, code)| code)
}

fn eval_into(
    ctx: &mut fend_core::Context,
    budget: &Budget,
    var: &str,
    expr: &str,
) -> Option<String> {
    let full = match ASSIGN.captures(expr) {
        Some(c) => format!("{} = ({}); {var} = {}", &c[1], &c[2], &c[1]),
        None => format!("{var} = ({expr})"),
    };
    let r = budget.evaluate(&full, ctx).ok()?;
    let out = r.get_main_result().trim().to_string();
    (!out.is_empty()).then_some(out)
}

fn eval_forgiving(
    ctx: &mut fend_core::Context,
    budget: &Budget,
    var: &str,
    expr: &str,
) -> Option<String> {
    let meaningful = expr
        .split(|c: char| !c.is_alphabetic())
        .any(|w| MEANINGFUL.contains(&w.to_ascii_lowercase().as_str()));
    if meaningful {
        return None;
    }
    drop_unknown_words(ctx, budget, var, expr).or_else(|| {
        if ASSIGN.is_match(expr) {
            return None;
        }
        let tokens: Vec<&str> = expr.split_whitespace().collect();
        let start = tokens
            .iter()
            .position(|t| t.starts_with(|c: char| c.is_ascii_digit() || "$£€¥₹(-.".contains(c)))?;

        if start == 0
            || tokens[start] == "-"
            || tokens[..start]
                .iter()
                .any(|t| t.contains(['+', '-', '*', '/', '^', '=', '%']) || t.starts_with("_l"))
        {
            return None;
        }
        drop_unknown_words(ctx, budget, var, &tokens[start..].join(" "))
    })
}

const MEANINGFUL: &[&str] = &[
    "until", "since", "ago", "before", "after", "every", "except", "without", "not",
];

fn drop_unknown_words(
    ctx: &mut fend_core::Context,
    budget: &Budget,
    var: &str,
    expr: &str,
) -> Option<String> {
    let mut expr = expr.to_string();
    let mut dropped = false;
    for _ in 0..12 {
        if dropped
            && !ASSIGN.is_match(&expr)
            && let Some(m) = SIDE_BY_SIDE.captures(&expr).and_then(|c| c.get(1))
        {
            expr = expr[m.start()..].to_string();
            continue;
        }
        let full = match ASSIGN.captures(&expr) {
            Some(c) => format!("{} = ({}); {var} = {}", &c[1], &c[2], &c[1]),
            None => format!("{var} = ({expr})"),
        };
        match budget.evaluate(&full, ctx) {
            Ok(r) => {
                let out = r.get_main_result().trim().to_string();
                return (!out.is_empty()).then_some(out);
            }
            Err(e) => {
                if std::env::var_os("OMACUS_DEBUG").is_some() {
                    eprintln!("{full:?} -> {e}");
                }
                let word = UNKNOWN_IDENT.captures(&e)?.get(1)?.as_str().to_string();
                if MEANINGFUL.contains(&word.to_ascii_lowercase().as_str()) {
                    return None;
                }
                let at = find_word(&expr, &word)?;

                if stuck_to_money(&expr[..at]) {
                    return None;
                }
                if qualifies_unit(&expr[at + word.len()..]) {
                    return None;
                }

                let before = expr[..at].trim_end();
                let after = expr[at + word.len()..].trim_start();
                if (before.is_empty() || before.ends_with(|c: char| "+-*/^×÷(=".contains(c)))
                    && after.starts_with(|c: char| "+-*/^×÷".contains(c))
                {
                    return None;
                }
                let next = format!("{} {}", &expr[..at], &expr[at + word.len()..]);
                if !next.chars().any(|c| c.is_ascii_alphanumeric()) {
                    return None;
                }
                expr = next;
                dropped = true;
            }
        }
    }
    None
}

fn stuck_to_money(before: &str) -> bool {
    if !before.ends_with(|c: char| c.is_ascii_digit() || c == ')' || "$£€¥₹₩₽₺".contains(c))
    {
        return false;
    }
    match before.rfind(['$', '£', '€', '¥', '₹', '₩', '₽', '₺']) {
        Some(i) => !before[i..].chars().any(char::is_whitespace),
        None => false,
    }
}

fn precisely(ctx: &mut fend_core::Context, budget: &Budget, var: &str, raw: String) -> String {
    let Some(body) = raw.strip_prefix("approx. ") else {
        return raw;
    };

    let unit_of = |b: &str| {
        b.split_once(' ')
            .map_or(String::new(), |(_, u)| u.to_string())
    };
    let raw_unit = unit_of(body);
    if raw_unit.split(' ').any(|t| {
        matches!(t, "+" | "-" | "i")
            || t.ends_with('i') && t.len() > 1 && t[..t.len() - 1].parse::<f64>().is_ok()
    }) || body.split(' ').next().is_some_and(|n| n.ends_with('i'))
    {
        return raw;
    }
    for shift in [20usize, 60] {
        let Ok(scaled) = budget.evaluate(&format!("({var}) * 1e{shift}"), ctx) else {
            return raw;
        };
        let scaled = scaled.get_main_result().trim().to_string();
        let body = scaled.trim_start_matches("approx. ");
        let (number, unit) = body.split_once(' ').unwrap_or((body, ""));
        if unit != raw_unit {
            return raw;
        }
        let (sign, digits) = match number.strip_prefix('-') {
            Some(d) => ("-", d),
            None => ("", number),
        };
        let (int, frac) = digits.split_once('.').unwrap_or((digits, ""));
        if int.is_empty() || !int.chars().chain(frac.chars()).all(|c| c.is_ascii_digit()) {
            return raw;
        }
        let int = int.trim_start_matches('0');
        if int.is_empty() && shift == 20 {
            continue;
        }
        let shifted = if int.len() > shift {
            format!(
                "{}.{}{frac}",
                &int[..int.len() - shift],
                &int[int.len() - shift..]
            )
        } else {
            format!("0.{}{int}{frac}", "0".repeat(shift - int.len()))
        };
        let unit = if unit.is_empty() {
            String::new()
        } else {
            format!(" {unit}")
        };
        return format!("{sign}{shifted}{unit}");
    }
    raw
}

fn find_word(s: &str, word: &str) -> Option<usize> {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    s.match_indices(word).map(|(i, _)| i).find(|&i| {
        let before = s[..i].chars().next_back().is_none_or(|c| !is_word(c));
        let after = s[i + word.len()..]
            .chars()
            .next()
            .is_none_or(|c| !is_word(c));
        before && after
    })
}

fn qualifies_unit(rest: &str) -> bool {
    let Some(next) = rest
        .split_whitespace()
        .next()
        .filter(|w| w.chars().all(|c| c.is_ascii_alphabetic()))
    else {
        return false;
    };
    let next = next.to_ascii_lowercase();
    if KEYWORDS.contains(&next.as_str()) || matches!(next.as_str(), "with" | "for" | "at") {
        return false;
    }
    thread_local! {
        static KNOWN: std::cell::RefCell<std::collections::HashMap<String, bool>> = Default::default();
    }
    KNOWN.with(|known| {
        *known.borrow_mut().entry(next.clone()).or_insert_with(|| {
            let mut ctx = fend_core::Context::new();
            fend_core::evaluate(&format!("1 {next}"), &mut ctx)
                .is_ok_and(|r| !r.get_main_result().trim().is_empty())
        })
    })
}

pub fn pretty(raw: &str) -> String {
    if let Some(view) = crate::engine::currency::money_view(raw) {
        return view;
    }
    let s = raw.trim_start_matches("approx. ");
    if looks_like_date(s) {
        return s.to_string();
    }
    let is_money = s.starts_with(['$', '£', '€', '¥', '₹'])
        || s.split_whitespace().any(|t| {
            t.ends_with('$') || (t.len() == 3 && t.chars().all(|c| c.is_ascii_uppercase()))
        });

    let dollars;
    let s = match s.strip_suffix(" USD") {
        Some(n) if NUM_TOKEN.is_match(n) && !n.starts_with('$') => {
            dollars = match n.strip_prefix('-') {
                Some(n) => format!("-${n}"),
                None => format!("${n}"),
            };
            dollars.as_str()
        }
        _ => s,
    };

    s.split(' ')
        .map(|tok| {
            let moved;
            let tok = match tok.strip_suffix('$') {
                Some(n) if NUM_TOKEN.is_match(n) => {
                    moved = format!("${n}");
                    moved.as_str()
                }
                _ => tok,
            };
            let tok = match tok {
                "celsius" => "°C",
                "fahrenheit" => "°F",
                _ => tok,
            };

            let (body, pct) = match (tok.strip_suffix('%'), tok.strip_suffix('i')) {
                (Some(b), _) => (b, "%"),
                (_, Some(b)) if NUM_TOKEN.is_match(b) => (b, "i"),
                _ => (tok, ""),
            };
            match NUM_TOKEN.captures(body) {
                Some(c) => {
                    let n = format_number(&c[4], c.get(5).map(|m| m.as_str()), is_money);
                    let sign = if c[1].is_empty() { &c[3] } else { &c[1] };
                    format!("{sign}{}{n}{pct}", &c[2])
                }
                None => tok.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
        .replacen("$-", "-$", 1)
        .replacen("£-", "-£", 1)
}

fn transfer_time(line: &str, raw: &str) -> Option<String> {
    let line = LABEL
        .captures(line)
        .map_or(line.to_string(), |c| c[2].to_string());
    if !TRANSFER.is_match(&line) {
        return None;
    }
    let raw = raw.trim_start_matches("approx. ");
    let secs: f64 = [" seconds", " second", " s"]
        .iter()
        .find_map(|unit| raw.strip_suffix(unit))?
        .replace(',', "")
        .parse()
        .ok()?;
    Some(human_duration(secs))
}

fn human_duration(secs: f64) -> String {
    if secs < 10.0 {
        let s = format!("{secs:.1}");
        return format!("{} s", s.trim_end_matches('0').trim_end_matches('.'));
    }
    let total = secs.round() as u64;
    let parts = [
        (total / 86_400, "day"),
        (total % 86_400 / 3_600, "h"),
        (total % 3_600 / 60, "min"),
        (total % 60, "s"),
    ];
    let shown: Vec<String> = parts
        .iter()
        .skip_while(|(n, _)| *n == 0)
        .take(2)
        .filter(|(n, _)| *n > 0)
        .map(|(n, unit)| match *unit {
            "day" if *n != 1 => format!("{n} days"),
            _ => format!("{n} {unit}"),
        })
        .collect();
    shown.join(" ")
}

fn looks_like_date(s: &str) -> bool {
    const NAMES: &[&str] = &[
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ];
    NAMES.iter().any(|n| s.contains(n))
}

pub(crate) fn format_number(int: &str, frac: Option<&str>, money: bool) -> String {
    let int = int.trim_start_matches('0');
    let frac = frac.unwrap_or("");
    let leading = frac.chars().take_while(|&c| c == '0').count();
    if int.is_empty() && leading == frac.len() {
        return "0".to_string();
    }
    if int.is_empty() && leading >= 6 {
        let (mut digits, carried) = round_digits(&frac[leading..], 3);
        digits.truncate(3);
        let exp = -(leading as i64) - 1 + carried as i64;
        let (first, rest) = digits.split_at(1);
        let rest = rest.trim_end_matches('0');
        return match rest {
            "" => format!("{first}e{exp}"),
            _ => format!("{first}.{rest}e{exp}"),
        };
    }
    let decimals = if !int.is_empty() || (money && leading < 2) {
        2
    } else {
        leading + 2
    };
    let (digits, _) = round_digits(&format!("{int}{frac}"), int.len() + decimals);
    let (i, f) = digits.split_at(digits.len() - decimals);
    let i = match i.trim_start_matches('0') {
        "" => "0",
        i => i,
    };
    let f = if money && decimals == 2 && !f.trim_end_matches('0').is_empty() {
        f
    } else {
        f.trim_end_matches('0')
    };
    let grouped = group_thousands(i);
    if f.is_empty() {
        grouped
    } else {
        format!("{grouped}.{f}")
    }
}

fn round_digits(digits: &str, keep: usize) -> (String, bool) {
    let mut out: Vec<u8> = digits.bytes().take(keep).collect();
    out.resize(keep, b'0');
    if digits.as_bytes().get(keep).is_some_and(|&d| d >= b'5') {
        for d in out.iter_mut().rev() {
            if *d == b'9' {
                *d = b'0';
            } else {
                *d += 1;
                return (String::from_utf8(out).unwrap_or_default(), false);
            }
        }
        out.insert(0, b'1');
        return (String::from_utf8(out).unwrap_or_default(), true);
    }
    (String::from_utf8(out).unwrap_or_default(), false)
}

fn shift_point(num: &str, places: usize) -> String {
    let (int, frac) = num.split_once('.').unwrap_or((num, ""));
    let mut frac = frac.to_string();
    if frac.len() < places {
        frac.push_str(&"0".repeat(places - frac.len()));
    }
    let (more, rest) = frac.split_at(places);
    let int = format!("{int}{more}");
    let int = match int.trim_start_matches('0') {
        "" => "0",
        i => i,
    };
    match rest {
        "" => int.to_string(),
        _ => format!("{int}.{rest}"),
    }
}

fn group_thousands(int: &str) -> String {
    let bytes = int.as_bytes();
    let mut out = String::with_capacity(int.len() + int.len() / 3);
    for (idx, b) in bytes.iter().enumerate() {
        if idx > 0 && (bytes.len() - idx).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval_results(doc: &str) -> Vec<LineResult> {
        Engine::new(Rates::fixed(&[
            ("USD", 1.0),
            ("EUR", 0.9),
            ("INR", 83.0),
            ("GBP", 0.78),
            ("BOB", 6.9),
            ("CHF", 0.9),
            ("BRL", 5.4),
            ("AED", 3.67),
        ]))
        .eval_document(doc)
    }

    fn eval(doc: &str) -> Vec<String> {
        eval_results(doc).into_iter().map(|r| r.display).collect()
    }

    fn one(line: &str) -> String {
        eval(line).pop().unwrap()
    }

    #[test]
    fn a_phone_keyboard_still_calculates() {
        assert_eq!(one("test=100"), "100");
        assert_eq!(eval("test=100\ntest*2"), vec!["100", "200"]);
        assert_eq!(one("test\u{00a0}=\u{00a0}100"), "100");
        assert_eq!(one("100 \u{2013} 20"), "80");
        assert_eq!(one("100 \u{2212} 20"), "80");
        assert_eq!(one("\u{201c}hi\u{201d}"), "");
    }

    #[test]
    fn hover_shows_a_variable_and_a_scaled_number() {
        let engine = Engine::new(Rates::fixed(&[("USD", 1.0)]));
        let text = "rent = $1,450\nrent * 2\n1.5k\n42";
        let use_at = text.rfind("rent").unwrap();
        let use_hover = engine
            .hover_at(text, text[..use_at].chars().count() + 1)
            .unwrap();
        assert_eq!(use_hover.value, "$1,450");
        let def = engine.hover_at(text, 1).unwrap();
        assert_eq!(def.value, "$1,450");
        assert_eq!(def.used_by, 1);
        let scaled = one("1.5k");
        assert_ne!(scaled, "1.5k");
        let number_at = text.find("1.5k").unwrap();
        let number = engine
            .hover_at(text, text[..number_at].chars().count())
            .unwrap();
        assert_eq!(number.value, scaled);
        let plain = text.find("42").unwrap();
        assert!(
            engine
                .hover_at(text, text[..plain].chars().count())
                .is_none()
        );

        let noted = "rent = $100 # rent";
        let comment = noted.rfind("rent").unwrap();

        assert_eq!(engine.hover_at(noted, 0).unwrap().used_by, 0);
        assert!(
            engine
                .hover_at(noted, noted[..comment].chars().count())
                .is_none()
        );
        let hotel = "- hotel = 3 nights * $180\nhotel";
        let again = hotel.rfind("hotel").unwrap();
        assert_eq!(
            engine
                .hover_at(hotel, hotel[..again].chars().count())
                .unwrap()
                .value,
            "$540"
        );
    }

    #[test]
    fn definition_span_is_the_name_after_a_prefix() {
        let line = "- hotel = 3 nights * $180";
        let (from, to) = definition_span(line).unwrap();
        assert_eq!(&line[from..to], "hotel");
        let noted = "rent = $100 # monthly";
        let (from, to) = definition_span(noted).unwrap();
        assert_eq!(&noted[from..to], "rent");
        assert!(definition_span("# Week 1").is_none());
        assert!(definition_span("// rent = 1").is_none());
    }

    #[test]
    fn name_hints_mark_unknown_names_only() {
        let rs = eval_results("a = 5\nA\ny = z + 1\nz = 4\nnope\nhello world\nUSD\n10 / 0");
        assert!(rs[1].hint);
        assert_eq!(rs[1].note.as_deref(), Some("Did you mean a?"));
        assert!(rs[2].hint);
        assert_eq!(rs[2].note.as_deref(), Some("z is defined below"));
        assert_eq!(rs[3].display, "4");
        assert!(rs[4].hint);
        assert_eq!(rs[4].note.as_deref(), Some("Unknown name 'nope'"));
        assert!(!rs[5].hint);
        assert_eq!(rs[5].note, None);
        assert!(!rs[6].hint);
        assert_eq!(rs[6].note, None);
        assert!(!rs[7].hint);
        assert_eq!(rs[7].note.as_deref(), Some("Can't divide by zero"));
    }

    #[test]
    fn percentages_in_prose_are_not_mod() {
        let prose = "Alex $90k, 0.5%, 1% share of sales + a bonus";
        let r = eval_results(prose).pop().unwrap();
        assert_eq!((r.display.as_str(), r.note.as_deref()), ("", None));
        let r = eval_results("7.5 % 2").pop().unwrap();
        assert_eq!(
            r.note.as_deref(),
            Some("mod needs whole numbers, 0 or more")
        );
    }

    #[test]
    fn realigns_answers_to_edited_text() {
        let old = "a = 1\nb = 2\nc = 3";
        let results = eval_results(old);
        let shown = |new: &str| -> Vec<String> {
            realign(old, &results, new)
                .into_iter()
                .map(|r| r.display)
                .collect()
        };

        assert_eq!(shown("a = 1\n\nb = 2\nc = 3"), ["1", "", "2", "3"]);
        assert_eq!(shown("new\na = 1\nb = 2\nc = 3"), ["", "1", "2", "3"]);

        assert_eq!(shown("a = 1\nc = 3"), ["1", "3"]);
        assert_eq!(shown("a = 1\nb = 20\nc = 3"), ["1", "", "3"]);
        assert_eq!(shown("a = 1\nb = 2\nc = 3\nd"), ["1", "2", "3", ""]);
        assert_eq!(shown(""), [""]);
        assert_eq!(shown(old), ["1", "2", "3"]);

        let old = "x\nx\nx";
        let results = eval_results("1\n2\n3");
        let got: Vec<String> = realign(old, &results, "x\nx")
            .into_iter()
            .map(|r| r.display)
            .collect();
        assert_eq!(got, ["1", "2"]);
    }

    #[test]
    fn answers_part_way_through_fill_in_over_the_old_ones() {
        let shown = eval_results("1\n2\n3");
        let text = "a = 5\nb = 6\nc = 7";
        let pairs = |rs: Vec<LineResult>| -> Vec<(String, bool)> {
            rs.into_iter().map(|r| (r.display, r.stale)).collect()
        };

        let part = &eval_results(text)[..1];
        assert_eq!(
            pairs(realign_onto(text, part, text, &shown)),
            [("5".into(), false), ("2".into(), true), ("3".into(), true)]
        );

        assert_eq!(
            pairs(realign_onto(text, &eval_results(text), text, &shown)),
            [
                ("5".into(), false),
                ("6".into(), false),
                ("7".into(), false)
            ]
        );
    }

    #[test]
    fn answers_below_an_edit_are_stale() {
        let old = "a = 1\nb = a + 1\nc = b + 1";
        let results = eval_results(old);
        let stale = |new: &str| -> Vec<bool> {
            realign(old, &results, new)
                .into_iter()
                .map(|r| r.stale)
                .collect()
        };

        assert_eq!(stale("a = 1\nb = a + 2\nc = b + 1"), [false, false, true]);
        assert_eq!(stale("a = 5\nb = a + 1\nc = b + 1"), [false, true, true]);
        assert_eq!(
            stale("a = 1\nb = a + 1\nc = b + 1\nd"),
            [false, false, false, false]
        );
        assert_eq!(stale(old), [false, false, false]);

        let moved = realign(old, &results, "a = 5\nb = a + 1\nc = b + 1");
        let again = realign(
            "a = 5\nb = a + 1\nc = b + 1",
            &moved,
            "a = 5\nb = a + 1\nc = b + 1\n",
        );
        assert_eq!(
            again.iter().map(|r| r.stale).collect::<Vec<_>>(),
            [false, true, true, false]
        );
    }

    #[test]
    fn arithmetic_and_formatting() {
        assert_eq!(one("2 + 2"), "4");
        assert_eq!(one("1200 * 12"), "14,400");
        assert_eq!(one("1/3"), "0.33");
        assert_eq!(one("0.000123 * 1"), "0.00012");
        assert_eq!(one("5 x 3"), "15");
        assert_eq!(one("10 times 3"), "30");
    }

    #[test]
    fn percentages() {
        assert_eq!(one("20% of 150"), "30");
        assert_eq!(one("5% on 200"), "210");
        assert_eq!(one("6% off 40"), "37.6");
        assert_eq!(one("50 as a % of 200"), "25%");
        assert_eq!(one("5% of what is 6"), "120");
        assert_eq!(one("6 is what % of 120"), "5%");
        assert_eq!(one("40 is 20% of what"), "200");
        assert_eq!(one("100 - 20%"), "80");
        assert_eq!(one("$50 + 8% tax"), "$54");
        assert_eq!(one("$1,000 + 7.5%"), "$1,075");
    }

    #[test]
    fn units_and_currency() {
        assert_eq!(one("3 km in miles"), "1.86 miles");
        assert_eq!(one("$30 in EUR"), "€27");
        assert_eq!(one("€9 in USD"), "$10");
        assert_eq!(one("rate = $85/hour\nrate * 6 hours * 4"), "$2,040");
        assert_eq!(one("$30 in EUR"), "€27");
        assert_eq!(one("$30 + 10 EUR"), "$41.11");
        assert_eq!(one("2 hours + 30 mins in minutes"), "150 minutes");
        assert_eq!(one("$85/hour"), "$85 / hour");
        assert_eq!(one("72 fahrenheit in celsius"), "22.22 °C");
        assert_eq!(one("60 km / 2 hours"), "30 km / hour");
        assert_eq!(one("2 liters / 250 ml"), "8");
        assert_eq!(one("1,200 km / 15 km/liter"), "80 liters");
        assert_eq!(one("$1,450 / 4 weeks"), "$362.50 / week");
        assert_eq!(one("1 nautical mile in km"), "1.85 km");
        assert_eq!(one("8 fl oz in ml"), "236.59 ml");
        assert_eq!(one("120 sq ft in m^2"), "11.15 m^2");
    }

    #[test]
    fn qualified_units_are_not_guessed() {
        assert_eq!(one("1 royal gallon in liters"), "");
        assert_eq!(one("Movie tickets 2 * $14"), "$28");
    }

    #[test]
    fn copies_a_sheet_with_answers() {
        let engine = Engine::new(Rates::fixed(&[("USD", 1.0)]));
        let text = "# Lunch\nPizza $24\nTip: 18% of prev\n\n";
        let copied = with_answers(text, &engine.eval_document(text));
        assert_eq!(
            copied,
            "# Lunch\nPizza $24           $24\nTip: 18% of prev    $4.32"
        );
    }

    #[test]
    fn numbers_in_labels_are_not_multiplied() {
        assert_eq!(one("Day 3 $50"), "$50");
        assert_eq!(one("Room 12 costs $50"), "$50");
        assert_eq!(one("iPhone 15 $799"), "$799");
        assert_eq!(one("Item 2: $5"), "$5");
        assert_eq!(one("Movie tickets 2 * $14"), "$28");
    }

    #[test]
    fn currency_scale_suffix_is_part_of_the_amount() {
        assert_eq!(one("$5m"), "$5,000,000");
        assert_eq!(one("$5 m"), "$5,000,000");
        assert_eq!(one("$1m"), "$1,000,000");
        assert_eq!(one("$1M"), "$1,000,000");
        assert_eq!(one("$1mm"), "$1,000,000");
        assert_eq!(one("$1MM"), "$1,000,000");
        assert_eq!(one("$180k"), "$180,000");
        assert_eq!(one("$180K"), "$180,000");
        assert_eq!(one("$2.5bn"), "$2,500,000,000");
        assert_eq!(one("$2.5b"), "$2,500,000,000");
        assert_eq!(one("$2.5B"), "$2,500,000,000");
        assert_eq!(one("$2.5BN"), "$2,500,000,000");
        assert_eq!(one("$10m + $1m"), "$11,000,000");
        assert_eq!(one("20% of $5m"), "$1,000,000");
        assert_eq!(one("$5m per month"), "$5,000,000 / month");
        assert_eq!(one("$2.4M / $1m per month"), "2.4 months");
        assert_eq!(one("$2.4m / $180k per month"), "13.33 months");
        assert_eq!(one("a=$1m at $10m pre"), "$11,000,000 post, 9.09%");
        assert_eq!(one("Food $5m"), "$5,000,000");
        assert_eq!(one("Seed: $5m"), "$5,000,000");
        assert_eq!(one("$5m in EUR"), "€4,500,000");
        assert_eq!(one("€5m"), "€5,000,000");
        assert_eq!(one("£5m"), "£5,000,000");
        assert_eq!(one("₹5m"), "₹5,000,000");

        assert_eq!(one("1m"), "1 m");
        assert_eq!(one("3mm"), "3 mm");
        assert_eq!(one("1.5B"), "1.5 B");
        assert_eq!(one("5km"), "5 km");
        assert_eq!(one("3MB"), "3 MB");

        assert_eq!(one("$5xyz"), "");
        assert_eq!(one("$5kg"), "");
        assert_eq!(one("$5km"), "");
        assert_eq!(one("€5xyz"), "");
        assert_ne!(one("$5m"), "$5");
        assert_ne!(one("$5kg"), "$5");
    }

    #[test]
    fn quantity_words_count() {
        assert_eq!(one("half of 50"), "25");
        assert_eq!(one("twice $40"), "$80");
        assert_eq!(one("a third of 90"), "30");
        assert_eq!(one("a quarter of $200"), "$50");
        assert_eq!(one("50 percent off 80"), "40");

        assert!(one("days until Dec 25").ends_with(" days"));
    }

    #[test]
    fn splits_tips_and_downloads() {
        assert_eq!(one("$84 split 3 ways"), "$28");
        assert_eq!(one("split $84 3 ways"), "$28");
        assert_eq!(one("Dinner: $240 split between 4 people"), "$60");
        assert_eq!(one("18% tip on $62"), "$73.16");
        assert_eq!(one("$62 + 18% tip"), "$73.16");
        assert_eq!(one("4 GB at 100 Mbps"), "5 min 20 s");
        assert_eq!(one("Game: 80 GB at 50 Mbps"), "3 h 33 min");
        assert_eq!(one("700 MB @ 10 MB/s"), "1 min 10 s");
        assert_eq!(one("10 MB at 100 Mbps"), "0.8 s");

        assert_eq!(one("3 pizzas at $12"), "$36");
    }

    #[test]
    fn colors_and_css_units() {
        let r = eval_results("#ff8800 in rgb\n#ff8800\n# Palette\n16px in rem");
        assert_eq!(r[0].display, "rgb(255, 136, 0)");
        assert_eq!(r[0].color, Some([255, 136, 0]));
        assert_eq!(r[1].kind, Kind::Value);
        assert_eq!(r[2].kind, Kind::Header);
        assert_eq!(r[3].display, "1 rem");
    }

    #[test]
    fn a_heading_needs_a_space_after_the_hashes() {
        let r = eval_results("#fff0 logo\n#roadtrip plans\n# Lunch\nPizza $24");
        assert_ne!(r[0].kind, Kind::Header, "glued hashes are not a heading");
        assert_ne!(r[1].kind, Kind::Header, "glued hashes are not a heading");
        assert_eq!(r[2].kind, Kind::Header);
        assert_eq!(r[3].display, "$24");
    }

    #[test]
    fn little_words_are_prose() {
        assert_eq!(one("I spent $40"), "$40");
        assert_eq!(one("A coffee $4"), "$4");
        assert_eq!(one("Coffee $4 a day"), "$4 / day");
        assert_eq!(one("3 pizzas at $12"), "$36");
        assert_eq!(one("Movie at 7pm"), "");
    }

    #[test]
    fn exact_big_and_visible_tiny_numbers() {
        assert_eq!(one("12345678901234567890"), "12,345,678,901,234,567,890");
        assert_eq!(one("1e-12"), "1e-12");
        assert_eq!(one("0.000123"), "0.00012");
        assert_eq!(one("$5 - $10"), "-$5");
        assert_eq!(one("0x10 + 1"), "0x11");
        assert_eq!(one("5 x 3"), "15");
    }

    #[test]
    fn division_keeps_conversions() {
        assert_eq!(one("$1200 / 12 in EUR"), "€90");
        assert_eq!(one("10 / 4 inches"), "2.5 inches");
        assert_eq!(one("60 km / 2 hours"), "30 km / hour");
    }

    #[test]
    fn prev_needs_a_line_above() {
        assert_eq!(one("prev * 2"), "");
    }

    #[test]
    fn version_is_prose() {
        assert_eq!(one("version"), "");
        assert_eq!(one("Pro version $40"), "$40");
    }

    #[test]
    fn imperial_measures() {
        assert_eq!(one("1 imperial gallon in liters"), "4.55 liters");
        assert_eq!(one("2 uk pints in ml"), "1,136.52 ml");
        assert_eq!(one("20 liters in imperial gallons"), "4.4 imperial gallons");
        assert_eq!(
            one("Fuel: 40 liters in imperial gallons"),
            "8.8 imperial gallons"
        );
        assert_eq!(one("1 imperial gallon in gallons"), "1.2 gallons");
        assert_eq!(one("1 pint in uk pints"), "0.83 imperial pints");
    }

    #[test]
    fn currency_code_before_amount() {
        assert_eq!(one("CHF 300 in USD"), "$333.33");
        assert_eq!(one("USD 30"), "$30");
    }

    #[test]
    fn notepad_conventions() {
        let doc = "# Budget\nRent: $1,200\nLunch with Bob $40 // friday\nsum\nprev * 2\n\n\"note\" 5\navg";
        assert_eq!(
            eval(doc),
            vec!["", "$1,200", "$40", "$1,240", "$2,480", "", "5", "5"]
        );
    }

    #[test]
    fn prose_with_symbols_and_named_totals() {
        let doc = "Food & drinks ₹2,000 * 4\nTaxi ₹800\nsubtotal = sum\ngst = 18% of subtotal\nsubtotal + gst";
        assert_eq!(
            eval(doc),
            vec!["₹8,000", "₹800", "₹8,800", "₹1,584", "₹10,384"]
        );
    }

    #[test]
    fn totals_do_not_count_themselves() {
        let doc = "Coffee $4.50\nTaxi $12\ntotal\naverage";
        assert_eq!(eval(doc), vec!["$4.50", "$12", "$16.50", "$8.25"]);
    }

    #[test]
    fn variables() {
        let doc = "rent = $1200\nrent * 12\nx = 5\nx + rent";
        let r = eval(doc);
        assert_eq!(r[0], "$1,200");
        assert_eq!(r[1], "$14,400");
        assert_eq!(r[2], "5");
    }

    #[test]
    fn grammar_forms_from_the_reference_calculator() {
        assert_eq!(one("15 and 9"), "24");
        assert_eq!(one("10 without 2"), "8");
        assert_eq!(one("20 divided by 4"), "5");
        assert_eq!(one("12 x 10"), "120");
        assert_eq!(one("5 thousand"), "5,000");
        assert_eq!(one("1.5 billion"), "1,500,000,000");
        assert_eq!(one("2k + 3k"), "5,000");
        assert_eq!(one("1 000 + 1"), "1,001");
        assert_eq!(one("10 in oct"), "12");
        assert_eq!(one("5300 in sci"), "5.30e3");
        assert_eq!(one("sqrt 16"), "4");
        assert_eq!(one("square root of 16"), "4");
        assert_eq!(one("cube root 27"), "3");
        assert_eq!(one("fact 5"), "120");
        assert_eq!(one("5!"), "120");
        assert_eq!(one("20% of what is 30"), "150");
        assert_eq!(one("5% on what is 105"), "100");
        assert_eq!(one("5% off what is 95"), "100");
        assert_eq!(one("50 pct. of 200"), "100");
        assert_eq!(one("25 percent of 80"), "20");
        assert_eq!(one("cubic root 27"), "3");
        assert_eq!(one("3 quadrillion"), "3,000,000,000,000,000");
        assert_eq!(one("255 in exponential"), "2.55e2");
        assert_eq!(one("1.5e3 + 1"), "1,501");
        assert_eq!(one("$1mm"), "$1,000,000");
        assert_eq!(one("5 lakh"), "500,000");
        assert_eq!(one("1 crore"), "10,000,000");
        assert_eq!(one("12 days in hours"), "288 hours");
        assert_eq!(one("1 mb to kb"), "1,000 kb");
        assert_eq!(one("1 megabyte in mb"), "8 megabits");
        assert_eq!(one("1 MB in KB"), "1,000 kB");
        assert_eq!(one("$100 + 10% - 5%"), "$104.50");
        assert_eq!(one("120 as a % increase of 100"), "20%");
        assert_eq!(one("80 as a % decrease of 100"), "20%");
        assert_eq!(one("50 INR in USD"), "$0.60");
        assert_eq!(one("R$10"), "R$10");
        assert_eq!(one("Dh 100 in USD"), "$27.25");
        assert_eq!(one("10 in"), "10 inches");
        assert_eq!(one("2 hours into minutes"), "120 minutes");
        assert_eq!(one("0b1010 + 1"), "0b1011");
        assert_eq!(
            eval(
                "x is 4
x += 3
x"
            )
            .pop()
            .unwrap(),
            "7"
        );
        assert_eq!(eval("y = 10\ny /= 4").pop().unwrap(), "2.5");
    }

    #[test]
    fn base_conversion_is_not_grouped() {
        assert_eq!(one("255 in binary"), "11111111");
        assert_eq!(one("255 in hex"), "ff");
    }

    #[test]
    fn money_multiplication_and_dates() {
        assert_eq!(one("3 * $95"), "$285");
        assert_eq!(one("Hotel 3 nights * $95"), "$285");
        let today = chrono::Local::now().format("%A, %-d %B %Y").to_string();
        assert_eq!(one("today"), today);
    }

    #[test]
    fn prose_is_ignored() {
        assert_eq!(one("hello world"), "");
        assert_eq!(one("10 apples + 5"), "15");
    }

    #[test]
    fn markdown_prefixes_compute_and_variables_cross_prose() {
        let doc = "- hotel = 3 nights * $180\npack sunscreen\nhotel\n# Next\nhotel * 2";
        let r = eval_results(doc);
        assert_eq!(r[0].display, "$540");
        assert_eq!(r[1].display, "");
        assert_eq!(r[2].display, "$540");
        assert_eq!(r[3].kind, Kind::Header);
        assert_eq!(r[4].display, "$1,080");
        assert!(defined_variables(doc).contains("hotel"));
        assert!(defined_variables("- [x] hotel = 1\n| a | b |\n").contains("hotel"));
        assert!(!defined_variables("| hotel = 1 |\n").contains("hotel"));
    }
}
