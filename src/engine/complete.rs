use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

const KEYWORDS: &[&str] = &[
    "prev",
    "sum",
    "total",
    "average",
    "min",
    "max",
    "today",
    "tomorrow",
    "yesterday",
    "now",
    "time",
    "in",
];

const COLOR_WORDS: &[&str] = &[
    "in",
    "to",
    "as",
    "of",
    "on",
    "off",
    "what",
    "is",
    "prev",
    "sum",
    "total",
    "avg",
    "average",
    "mean",
    "min",
    "max",
    "today",
    "now",
    "tomorrow",
    "yesterday",
    "plus",
    "minus",
    "times",
    "mod",
    "time",
];

const MAX: usize = 10;

const FUNCTIONS: &[(&str, &str)] = &[
    ("sqrt", "square root"),
    ("cbrt", "cube root"),
    ("abs", "distance from zero"),
    ("round", "nearest whole number"),
    ("ceil", "up to a whole number"),
    ("floor", "down to a whole number"),
    ("ln", "log base e"),
    ("log", "log base 10"),
    ("fact", "factorial"),
    ("sin", "sine"),
    ("cos", "cosine"),
    ("tan", "tangent"),
    ("asin", "inverse sine"),
    ("acos", "inverse cosine"),
    ("atan", "inverse tangent"),
    ("sinh", "hyperbolic sine"),
    ("cosh", "hyperbolic cosine"),
    ("tanh", "hyperbolic tangent"),
];

const CONSTANTS: &[(&str, &str)] = &[
    ("pi", "3.14159…"),
    ("tau", "6.28318…"),
    ("phi", "1.61803…"),
    ("e", "2.71828…"),
];

const SCALES: &[(&str, &str)] = &[
    ("k", "×1,000"),
    ("M", "×1,000,000"),
    ("thousand", "×1,000"),
    ("million", "×1,000,000"),
    ("billion", "×1,000,000,000"),
    ("trillion", "×1,000,000,000,000"),
    ("lakh", "×100,000"),
    ("crore", "×10,000,000"),
];

const CURRENCIES: &[(&str, &str)] = &[
    ("USD", "US dollar"),
    ("EUR", "euro"),
    ("GBP", "pound"),
    ("INR", "rupee"),
    ("JPY", "yen"),
    ("CNY", "yuan"),
    ("AUD", "Australian dollar"),
    ("CAD", "Canadian dollar"),
    ("CHF", "Swiss franc"),
    ("BRL", "real"),
    ("MXN", "Mexican peso"),
    ("SGD", "Singapore dollar"),
    ("SEK", "Swedish krona"),
    ("NOK", "Norwegian krone"),
    ("DKK", "Danish krone"),
    ("KRW", "won"),
    ("RUB", "ruble"),
    ("TRY", "lira"),
    ("PLN", "zloty"),
    ("AED", "dirham"),
    ("SAR", "riyal"),
    ("ZAR", "rand"),
    ("ILS", "shekel"),
    ("THB", "baht"),
];

const AGGREGATES: &[(&str, &str)] = &[
    ("sum", "lines since a blank line or heading"),
    ("total", "lines since a blank line or heading"),
    ("average", "the average of those lines"),
    ("avg", "the average of those lines"),
    ("mean", "the average of those lines"),
    ("min", "the smallest of those lines"),
    ("max", "the largest of those lines"),
];

static CURRENCY_WORDS: LazyLock<HashMap<String, &'static str>> = LazyLock::new(|| {
    let mut words = HashMap::new();
    for (code, name) in CURRENCIES {
        for word in name.split_whitespace() {
            let word = word.to_lowercase();
            if word.len() < 3 || word == "pound" {
                continue;
            }

            words.entry(word.clone()).or_insert(*code);
            words.entry(format!("{word}s")).or_insert(*code);
        }
    }
    words
});

fn currency_word(word: &str) -> Option<&'static str> {
    CURRENCY_WORDS.get(word).copied()
}

fn currency_name(code: &str) -> &'static str {
    CURRENCIES
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, name)| *name)
        .unwrap_or("")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub text: String,
    pub kind: &'static str,
    pub detail: String,
}

impl Suggestion {
    pub fn kind_label(&self) -> &'static str {
        match self.kind {
            "zone" => "time zone",
            "aggregate" => "total",
            "const" => "constant",
            "currency" => "currency",
            other => other,
        }
    }

    pub fn meta(&self) -> String {
        if self.detail.is_empty() {
            self.kind_label().to_string()
        } else {
            format!("{} · {}", self.kind_label(), self.detail)
        }
    }

    pub fn is_function(&self) -> bool {
        self.kind == "function"
    }
}

pub fn suggest(prefix: &str, vars: &HashSet<String>) -> Vec<Suggestion> {
    const STOP: &[&str] = &["in", "to", "as", "of", "on", "at", "is", "by", "per", "off"];
    if prefix.chars().count() < 2 || STOP.contains(&prefix.to_lowercase().as_str()) {
        return Vec::new();
    }
    let lower = prefix.to_lowercase();
    let starts = |w: &str| w.to_lowercase().starts_with(&lower) && w.len() > prefix.len();

    let mut names: Vec<String> = vars.iter().filter(|v| starts(v)).cloned().collect();
    names.sort();

    let mut units: Vec<String> = Vec::new();
    for probe in [lower.as_str(), prefix] {
        for item in fend_core::get_completions_for_prefix(probe).1 {
            let word = item.display().to_string();
            if !units.contains(&word) {
                units.push(word);
            }
        }
    }

    let mut candidates = Vec::new();
    for name in names {
        candidates.push(Suggestion {
            text: name,
            kind: "variable",
            detail: "a name on this page".to_string(),
        });
    }
    for (name, detail) in FUNCTIONS.iter().filter(|(name, _)| starts(name)) {
        candidates.push(Suggestion {
            text: name.to_string(),
            kind: "function",
            detail: (*detail).to_string(),
        });
    }
    for (name, detail) in AGGREGATES.iter().filter(|(name, _)| starts(name)) {
        candidates.push(Suggestion {
            text: name.to_string(),
            kind: "aggregate",
            detail: (*detail).to_string(),
        });
    }
    for (name, detail) in CONSTANTS.iter().filter(|(name, _)| starts(name)) {
        candidates.push(Suggestion {
            text: name.to_string(),
            kind: "const",
            detail: (*detail).to_string(),
        });
    }
    for (name, detail) in SCALES.iter().filter(|(name, _)| starts(name)) {
        candidates.push(Suggestion {
            text: name.to_string(),
            kind: "scale",
            detail: (*detail).to_string(),
        });
    }
    for (code, name) in CURRENCIES.iter().filter(|(code, _)| starts(code)) {
        candidates.push(Suggestion {
            text: code.to_string(),
            kind: "currency",
            detail: (*name).to_string(),
        });
    }
    for word in KEYWORDS.iter().copied().filter(|k| starts(k)) {
        candidates.push(Suggestion {
            text: word.to_string(),
            kind: "keyword",
            detail: keyword_detail(word).to_string(),
        });
    }
    for zone in crate::engine::time::completion_words().filter(|z| starts(z)) {
        candidates.push(Suggestion {
            text: zone.to_string(),
            kind: "zone",
            detail: crate::engine::time::completion_meaning(zone)
                .map(|iana| iana.rsplit('/').next().unwrap_or(iana).replace('_', " "))
                .unwrap_or_default(),
        });
    }
    let mut seen: HashSet<String> = candidates
        .iter()
        .map(|item| item.text.to_lowercase())
        .collect();
    for unit in units.iter().filter(|u| starts(u)) {
        let lower = unit.to_lowercase();

        if let Some(code) = currency_word(&lower) {
            if seen.insert(code.to_lowercase()) {
                candidates.push(Suggestion {
                    text: code.to_string(),
                    kind: "currency",
                    detail: currency_name(code).to_string(),
                });
            }
            continue;
        }
        if seen.contains(&lower)
            || (lower.ends_with('s') && seen.contains(lower.trim_end_matches('s')))
        {
            continue;
        }
        seen.insert(lower);
        candidates.push(Suggestion {
            text: unit.clone(),
            kind: "unit",
            detail: String::new(),
        });
    }

    let mut out = Vec::new();
    for item in candidates {
        if out.iter().any(|seen: &Suggestion| seen.text == item.text) {
            continue;
        }
        out.push(item);
        if out.len() == MAX {
            break;
        }
    }
    out
}

pub fn word_kind(word: &str) -> Option<&'static str> {
    let lower = word.to_ascii_lowercase();
    if FUNCTIONS.iter().any(|(name, _)| *name == lower) {
        return Some("function");
    }
    if CONSTANTS.iter().any(|(name, _)| *name == lower) {
        return Some("constant");
    }
    if SCALES.iter().any(|(name, _)| *name == lower) {
        return Some("scale");
    }
    if AGGREGATES.iter().any(|(name, _)| *name == lower) {
        return Some("aggregate");
    }
    if COLOR_WORDS.contains(&lower.as_str()) {
        return Some("keyword");
    }
    if ZONES.contains(&lower) {
        return Some("zone");
    }
    if UNITS.contains(&lower) {
        return Some("unit");
    }
    None
}

fn keyword_detail(word: &str) -> &'static str {
    match word {
        "prev" => "the line above",
        "sum" | "total" => "lines since a blank line or heading",
        "average" => "the average of those lines",
        "min" => "the smallest value",
        "max" => "the largest value",
        "today" => "today's date",
        "tomorrow" => "tomorrow's date",
        "yesterday" => "yesterday's date",
        "now" => "the time now",
        "time" => "the time in a place",
        "in" => "convert into",
        _ => "a keyword",
    }
}

static ZONES: LazyLock<HashSet<String>> = LazyLock::new(|| {
    crate::engine::time::completion_words()
        .map(|z| z.to_ascii_lowercase())
        .collect()
});

static UNITS: LazyLock<HashSet<String>> = LazyLock::new(|| {
    let mut set = HashSet::new();
    for letter in 'a'..='z' {
        let prefix = letter.to_string();
        for unit in fend_core::get_completions_for_prefix(&prefix).1 {
            set.insert(unit.display().to_ascii_lowercase());
        }
    }
    set
});

pub fn word_before(text: &str, cursor: usize) -> Option<(usize, String)> {
    let chars: Vec<char> = text.chars().collect();
    if cursor > chars.len() {
        return None;
    }

    if chars
        .get(cursor)
        .is_some_and(|c| c.is_alphanumeric() || *c == '_')
    {
        return None;
    }
    let mut start = cursor;
    while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
        start -= 1;
    }
    let word: String = chars[start..cursor].iter().collect();

    if !word.chars().next().is_some_and(char::is_alphabetic) {
        return None;
    }
    let line_start = chars[..start]
        .iter()
        .rposition(|&c| c == '\n')
        .map_or(0, |i| i + 1);
    let line: String = chars[line_start..start].iter().collect();
    if line.trim_start().starts_with('#')
        || line.contains("//")
        || line.matches('"').count() % 2 == 1
    {
        return None;
    }
    Some((start, word))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggests_vars_units_and_zones() {
        let vars = HashSet::from(["rent".to_string(), "rate".to_string()]);
        let first = &suggest("re", &vars)[0];
        assert_eq!(first.text, "rent");
        assert_eq!(first.kind, "variable");
        assert_eq!(first.detail, "a name on this page");
        let mile = suggest("mil", &vars)
            .into_iter()
            .find(|s| s.text == "mile")
            .expect("mile");
        assert_eq!(mile.kind, "unit");
        assert_eq!(mile.detail, "");
        assert!(
            suggest("fahr", &vars)
                .iter()
                .any(|s| s.text == "fahrenheit")
        );
        let pst = suggest("PS", &vars)
            .into_iter()
            .find(|s| s.text == "PST")
            .expect("PST");
        assert_eq!(pst.kind, "zone");
        assert_eq!(pst.detail, "Los Angeles");
        let tomorrow = suggest("tomo", &vars)
            .into_iter()
            .find(|s| s.text == "tomorrow")
            .expect("tomorrow");
        assert_eq!(tomorrow.kind, "keyword");
        assert_eq!(tomorrow.detail, "tomorrow's date");
        assert!(suggest("r", &vars).is_empty());
        assert!(suggest("in", &vars).is_empty());
    }

    #[test]
    fn a_currency_word_becomes_its_code_whatever_you_type() {
        for prefix in ["eu", "eur", "EU", "Eur"] {
            let items = suggest(prefix, &HashSet::new());
            assert!(
                items
                    .iter()
                    .any(|s| s.text == "EUR" && s.kind == "currency"),
                "{prefix} should offer EUR: {items:?}"
            );
            assert!(
                !items.iter().any(|s| s.text.eq_ignore_ascii_case("euro")),
                "{prefix} offered the euro twice: {items:?}"
            );
        }
        let dollars = suggest("doll", &HashSet::new());
        assert!(
            dollars
                .iter()
                .any(|s| s.text == "USD" && s.kind == "currency"),
            "dollar should offer USD: {dollars:?}"
        );
        assert!(
            !dollars.iter().any(|s| s.text.starts_with("dollar")),
            "dollar offered twice: {dollars:?}"
        );
    }

    #[test]
    fn a_weight_stays_a_weight() {
        let items = suggest("pou", &HashSet::new());
        assert!(
            items.iter().any(|s| s.text == "pound" && s.kind == "unit"),
            "pound is a unit of mass, not a currency: {items:?}"
        );
    }

    #[test]
    fn one_thing_gets_one_suggestion() {
        let items = suggest("eu", &HashSet::new());
        let currency = items.iter().find(|s| s.text == "EUR").expect("EUR");
        assert_eq!(currency.detail, "euro");
        assert!(
            !items.iter().any(|s| s.text.eq_ignore_ascii_case("euro")),
            "the euro is offered twice: {items:?}"
        );
        let miles = suggest("mi", &HashSet::new());
        assert!(
            miles.iter().any(|s| s.text == "mile"),
            "mile should still be suggested: {miles:?}"
        );
        assert!(
            !miles.iter().any(|s| s.text == "miles"),
            "plurals repeat the singular: {miles:?}"
        );
    }

    #[test]
    fn the_meta_label_never_repeats_the_kind() {
        let mile = suggest("mil", &HashSet::new())
            .into_iter()
            .find(|s| s.text == "mile")
            .expect("mile");
        assert_eq!(mile.meta(), "unit");
        let eur = suggest("eu", &HashSet::new())
            .into_iter()
            .find(|s| s.text == "EUR")
            .expect("EUR");
        assert_eq!(eur.meta(), "currency · euro");
    }

    #[test]
    fn colours_units_zones_and_keywords() {
        assert_eq!(word_kind("mile"), Some("unit"));
        assert_eq!(word_kind("PST"), Some("zone"));
        assert_eq!(word_kind("total"), Some("aggregate"));
        assert_eq!(word_kind("sqrt"), Some("function"));
        assert_eq!(word_kind("pi"), Some("constant"));
        assert_eq!(word_kind("million"), Some("scale"));
        assert_eq!(word_kind("rent"), None);
    }

    #[test]
    fn suggestions_cover_functions_units_and_currencies() {
        let vars = std::collections::HashSet::new();
        let found = |kind: &str, prefix: &str, text: &str| {
            suggest(prefix, &vars)
                .iter()
                .any(|s| s.kind == kind && s.text == text)
        };
        assert!(found("function", "sq", "sqrt"));
        assert!(found("function", "asi", "asin"));
        assert!(found("aggregate", "ave", "average"));
        assert!(found("const", "ta", "tau"));
        assert!(found("scale", "bil", "billion"));
        assert!(found("currency", "eu", "EUR"));
        assert!(found("unit", "mil", "mile"));
        assert!(found("zone", "ps", "PST"));
        let square = suggest("sq", &vars).remove(0);
        assert!(square.is_function());
        assert_eq!(square.kind_label(), "function");
    }
    #[test]
    fn finds_word_at_cursor() {
        assert_eq!(word_before("3 km in mi", 10), Some((8, "mi".into())));
        assert_eq!(word_before("x = 1.5", 7), None);
        assert_eq!(word_before("// note", 7), None);
        assert_eq!(word_before("rent\nre", 7), Some((5, "re".into())));
    }
}
