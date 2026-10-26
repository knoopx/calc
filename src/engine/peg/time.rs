//! Ordered-choice parsers for date, time, duration and zone phrases.
//!
//! Every matcher here is a hand-written scanner over characters plus recursive
//! descent with explicit ordered choice and backtracking: no regular
//! expressions are used, and the phrase shapes live in code the way the
//! arithmetic grammar lives in `lexer.rs` and `parse.rs`.

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

const WEEKDAYS: [(&str, u32); 16] = [
    ("monday", 0),
    ("mon", 0),
    ("tuesday", 1),
    ("tues", 1),
    ("tue", 1),
    ("wednesday", 2),
    ("wed", 2),
    ("thursday", 3),
    ("thurs", 3),
    ("thu", 3),
    ("friday", 4),
    ("fri", 4),
    ("saturday", 5),
    ("sat", 5),
    ("sunday", 6),
    ("sun", 6),
];

const SHIFT_UNITS: &[&str] = &[
    "weeks", "week", "days", "day", "hours", "hour", "hrs", "hr", "h", "minutes", "minute", "mins",
    "min",
];
const EXPR_UNITS: &[&str] = &[
    "minutes", "minute", "mins", "min", "hours", "hour", "hrs", "hr", "days", "day", "weeks",
    "week", "months", "month", "years", "year",
];
const CAL_UNITS: &[&str] = &[
    "days", "day", "weeks", "week", "months", "month", "years", "year",
];
const SPAN_UNITS: &[&str] = &[
    "days",
    "weeks",
    "months",
    "workdays",
    "work days",
    "weekdays",
    "business days",
];
const COUNT_UNITS: &[&str] = &["days", "workdays", "work days", "weekdays", "business days"];
const BETWEEN_UNITS: &[&str] = &[
    "days",
    "weeks",
    "workdays",
    "work days",
    "weekdays",
    "business days",
];
const RELATIONS: &[&str] = &["until", "till", "til", "before", "since"];
const PAIRS: &[&str] = &["between", "from"];
const JOINS: &[&str] = &["and", "to", "until", "till"];
const IN_AT: &[&str] = &["in", "at"];
const IN_TO: &[&str] = &["in", "to"];
const CONVERT_TO: &[&str] = &["in", "to", "as"];
const HEADINGS: &[&str] = &["how many", "how long"];
const HOW_MANY: &[&str] = &["how many"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unit {
    Minute,
    Hour,
    Day,
    Week,
    Month,
    Year,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpanUnit {
    Week,
    Day,
    Hour,
    Minute,
}

#[derive(Clone, Copy, Debug)]
pub struct Amount {
    pub n: f64,
    pub unit: SpanUnit,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Base {
    Today,
    Now,
    Tomorrow,
    Yesterday,
}

pub struct Basic {
    pub base: Base,
    pub offset: Option<(bool, i64, Unit)>,
}

pub struct ZoneNow {
    pub zone: String,
}

pub struct Convert {
    pub from: String,
    pub zone: String,
}

pub struct FromNow {
    pub amounts: Vec<Amount>,
    pub zone: Option<String>,
}

pub struct Shift {
    pub from: String,
    pub minus: bool,
    pub amounts: Vec<Amount>,
    pub zone: Option<String>,
}

pub struct DateShift {
    pub from: String,
    pub minus: bool,
    pub n: i64,
    pub unit: Unit,
}

pub struct DateDiff {
    pub a: String,
    pub b: String,
}

pub struct Until {
    pub unit: Option<String>,
    pub until: bool,
    pub date: String,
}

pub struct Between {
    pub unit: String,
    pub a: String,
    pub b: String,
}

pub struct MonthCount {
    pub unit: String,
    pub month: u32,
    pub year: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DatePhrase {
    Offset(i64),
    Weekday {
        day: u32,
        next: bool,
    },
    MonthDay {
        month: u32,
        day: u32,
        year: Option<u32>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slash {
    pub a: u32,
    pub b: u32,
    pub year: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Clock {
    pub word: Option<String>,
    pub hour: Option<u32>,
    pub minute: Option<u32>,
    pub meridiem: Option<String>,
    pub rest: String,
}

struct Run {
    start: usize,
    end: usize,
    word: String,
}

fn chars(text: &str) -> Vec<char> {
    text.chars().collect()
}

fn wordish(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn runs(c: &[char]) -> Vec<Run> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < c.len() {
        if wordish(c[at]) {
            let start = at;
            while at < c.len() && wordish(c[at]) {
                at += 1;
            }
            out.push(Run {
                start,
                end: at,
                word: c[start..at].iter().collect::<String>().to_lowercase(),
            });
        } else {
            at += 1;
        }
    }
    out
}

fn slice(c: &[char], from: usize, to: usize) -> String {
    let (from, to) = (from.min(c.len()), to.min(c.len()));
    c[from..to].iter().collect::<String>().trim().to_string()
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

struct Cur {
    c: Vec<char>,
    at: usize,
}

impl Cur {
    fn new(text: &str) -> Self {
        Cur {
            c: chars(text),
            at: 0,
        }
    }

    fn at(c: &[char], from: usize) -> Self {
        Cur {
            c: c.to_vec(),
            at: from,
        }
    }

    fn mark(&self) -> usize {
        self.at
    }

    fn back(&mut self, at: usize) {
        self.at = at;
    }

    fn eof(&self) -> bool {
        self.at >= self.c.len()
    }

    fn peek(&self) -> Option<char> {
        self.c.get(self.at).copied()
    }

    fn ws(&mut self) {
        while self.at < self.c.len() && self.c[self.at].is_whitespace() {
            self.at += 1;
        }
    }

    fn eat(&mut self, ch: char) -> bool {
        if self.peek() == Some(ch) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn word(&mut self) -> Option<String> {
        let start = self.at;
        while self.at < self.c.len() && wordish(self.c[self.at]) {
            self.at += 1;
        }
        if start == self.at {
            return None;
        }
        Some(
            self.c[start..self.at]
                .iter()
                .collect::<String>()
                .to_lowercase(),
        )
    }

    fn letters(&mut self) -> Option<String> {
        let start = self.at;
        while self.at < self.c.len() && self.c[self.at].is_alphabetic() {
            self.at += 1;
        }
        if start == self.at {
            return None;
        }
        Some(
            self.c[start..self.at]
                .iter()
                .collect::<String>()
                .to_lowercase(),
        )
    }

    fn digits(&mut self) -> Option<String> {
        let start = self.at;
        while self.at < self.c.len() && self.c[self.at].is_ascii_digit() {
            self.at += 1;
        }
        if start == self.at {
            return None;
        }
        Some(self.c[start..self.at].iter().collect::<String>())
    }

    fn bounded(&self) -> bool {
        self.eof() || !wordish(self.c[self.at])
    }

    fn done(&mut self) -> bool {
        self.ws();
        self.eof()
    }

    fn rest(&self) -> String {
        slice(&self.c, self.at, self.c.len())
    }
}

fn number(cur: &mut Cur) -> Option<f64> {
    let whole = cur.digits()?;
    let mut text = whole;
    if cur.peek() == Some('.') && cur.c.get(cur.at + 1).is_some_and(|c| c.is_ascii_digit()) {
        cur.at += 1;
        text.push('.');
        text.push_str(&cur.digits()?);
    }
    text.parse().ok()
}

fn digits_upto(cur: &mut Cur, max: usize) -> Option<u32> {
    let start = cur.mark();
    let text = cur.digits()?;
    if text.len() > max || !cur.bounded() {
        cur.back(start);
        return None;
    }
    text.parse().ok()
}

fn unit_of(word: &str, table: &[&str]) -> bool {
    table.contains(&word)
}

fn expr_unit(word: &str) -> Option<Unit> {
    if !unit_of(word, EXPR_UNITS) {
        return None;
    }
    Some(match word.trim_end_matches('s') {
        "minute" | "min" => Unit::Minute,
        "hour" | "hr" => Unit::Hour,
        "day" => Unit::Day,
        "week" => Unit::Week,
        "month" => Unit::Month,
        _ => Unit::Year,
    })
}

fn cal_unit(word: &str) -> Option<Unit> {
    if !unit_of(word, CAL_UNITS) {
        return None;
    }
    Some(match word.trim_end_matches('s') {
        "day" => Unit::Day,
        "week" => Unit::Week,
        "month" => Unit::Month,
        _ => Unit::Year,
    })
}

fn span_unit(word: &str) -> Option<SpanUnit> {
    if !unit_of(word, SHIFT_UNITS) {
        return None;
    }
    Some(match word {
        "weeks" | "week" => SpanUnit::Week,
        "days" | "day" => SpanUnit::Day,
        "minutes" | "minute" | "mins" | "min" => SpanUnit::Minute,
        _ => SpanUnit::Hour,
    })
}

fn month_of(word: &str) -> Option<u32> {
    MONTHS
        .iter()
        .position(|m| word.starts_with(m))
        .map(|i| i as u32 + 1)
}

fn weekday_of(word: &str) -> Option<u32> {
    WEEKDAYS.iter().find(|(n, _)| *n == word).map(|(_, d)| *d)
}

fn match_phrase(found: &[Run], at: usize, table: &[&str]) -> Option<(usize, usize, String)> {
    if at >= found.len() {
        return None;
    }
    let mut best: Option<(usize, usize, String)> = None;
    for phrase in table {
        let parts: Vec<&str> = phrase.split_whitespace().collect();
        if at + parts.len() > found.len() {
            continue;
        }
        if !found[at..at + parts.len()]
            .iter()
            .enumerate()
            .all(|(k, run)| run.word == parts[k])
        {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|(taken, _, _)| parts.len() > *taken)
        {
            best = Some((
                parts.len(),
                found[at + parts.len() - 1].end,
                parts.join(" "),
            ));
        }
    }
    best
}

fn find_split(text: &str, table: &[&str]) -> Option<(String, String, String)> {
    let c = chars(text.trim());
    let found = runs(&c);
    for at in 0..found.len() {
        let Some((_taken, end, word)) = match_phrase(&found, at, table) else {
            continue;
        };
        let start = found[at].start;
        if start == 0 || !c[start - 1].is_whitespace() {
            continue;
        }
        if end >= c.len() || !c[end].is_whitespace() {
            continue;
        }
        let lhs = slice(&c, 0, start);
        let rhs = slice(&c, end, c.len());
        if lhs.is_empty() || rhs.is_empty() {
            continue;
        }
        return Some((lhs, word, rhs));
    }
    None
}

fn amounts(text: &str) -> Option<(Vec<Amount>, String)> {
    let mut cur = Cur::new(text);
    let mut out = Vec::new();
    loop {
        cur.ws();
        let save = cur.mark();
        let Some(n) = number(&mut cur) else { break };
        cur.ws();
        let Some(word) = cur.word() else {
            cur.back(save);
            break;
        };
        let Some(unit) = span_unit(&word) else {
            cur.back(save);
            break;
        };
        if !cur.bounded() {
            cur.back(save);
            break;
        }
        out.push(Amount { n, unit });
    }
    if out.is_empty() {
        return None;
    }
    Some((out, cur.rest()))
}

fn zone_tail(left: &str, table: &[&str], strip: bool) -> Option<Option<String>> {
    if left.is_empty() {
        return Some(None);
    }
    let c = chars(left.trim());
    let found = runs(&c);
    for at in 0..found.len() {
        let Some((_, end, _)) = match_phrase(&found, at, table) else {
            continue;
        };
        if at > 0 && !c[found[at].start - 1].is_whitespace() {
            continue;
        }
        let rest = slice(&c, end, c.len());
        let rest = if strip { strip_query(&rest) } else { rest };
        if rest.is_empty() {
            continue;
        }
        return Some(Some(rest));
    }
    None
}

fn cal_amount(text: &str) -> Option<(i64, Unit)> {
    let mut cur = Cur::new(text);
    let digits = cur.digits()?;
    let n: i64 = digits.parse().ok()?;
    cur.ws();
    let word = cur.word()?;
    let unit = cal_unit(&word)?;
    if !cur.done() {
        return None;
    }
    Some((n, unit))
}

pub fn basic(line: &str) -> Option<Basic> {
    let mut cur = Cur::new(line.trim());
    let word = cur.word()?;
    let base = match word.as_str() {
        "today" => Base::Today,
        "now" => Base::Now,
        "tomorrow" => Base::Tomorrow,
        "yesterday" => Base::Yesterday,
        _ => return None,
    };
    if !cur.bounded() {
        return None;
    }
    if cur.done() {
        return Some(Basic { base, offset: None });
    }
    cur.ws();
    let minus = match cur.peek() {
        Some('+') => false,
        Some('-') => true,
        _ => return None,
    };
    cur.at += 1;
    cur.ws();
    let n: i64 = cur.digits()?.parse().ok()?;
    cur.ws();
    let unit = expr_unit(&cur.word()?)?;
    if !cur.done() {
        return None;
    }
    Some(Basic {
        base,
        offset: Some((minus, n, unit)),
    })
}

pub fn zone_now(line: &str) -> Option<ZoneNow> {
    let mut cur = Cur::new(line.trim());
    let save = cur.mark();
    if cur.word().is_some_and(|w| w == "what") {
        if !cur.peek().is_some_and(|c| c.is_whitespace()) {
            cur.back(save);
        } else {
            cur.ws();
        }
    } else {
        cur.back(save);
    }
    let head = cur.word()?;
    if head != "time" && head != "now" {
        return None;
    }
    if !cur.peek().is_some_and(|c| c.is_whitespace()) {
        return None;
    }
    cur.ws();
    let link = cur.word()?;
    if link != "in" && link != "at" {
        return None;
    }
    if !cur.peek().is_some_and(|c| c.is_whitespace()) {
        return None;
    }
    cur.ws();
    let mut zone = cur.rest();
    if zone.is_empty() {
        return None;
    }
    if let Some(short) = zone.strip_suffix('?') {
        zone = short.trim().to_string();
        if zone.is_empty() {
            return None;
        }
    }
    Some(ZoneNow { zone })
}

pub fn convert(line: &str) -> Option<Convert> {
    let (from, _, zone) = find_split(line.trim(), CONVERT_TO)?;
    Some(Convert { from, zone })
}

pub fn from_now(line: &str) -> Option<FromNow> {
    let line = line.trim();
    let mut cur = Cur::new(line);
    let save = cur.mark();
    let lead =
        cur.word().is_some_and(|w| w == "in") && cur.peek().is_some_and(|c| c.is_whitespace());
    if lead {
        cur.ws();
        let (list, left) = amounts(&cur.rest())?;
        let zone = zone_tail(&left, IN_AT, true)?;
        return Some(FromNow {
            amounts: list,
            zone,
        });
    }
    cur.back(save);
    let c = chars(line);
    let found = runs(&c);
    for at in 0..found.len() {
        let Some((taken, _, _)) = match_phrase(&found, at, &["from now"]) else {
            continue;
        };
        let head = slice(&c, 0, found[at].start);
        let Some(list) = amounts(&head) else {
            continue;
        };
        let (list, left) = list;
        if !left.is_empty() {
            continue;
        }
        let left = slice(&c, found[at + taken - 1].end, c.len());
        let Some(zone) = zone_tail(&left, IN_AT, true) else {
            continue;
        };
        return Some(FromNow {
            amounts: list,
            zone,
        });
    }
    None
}

pub fn shift(line: &str) -> Option<Shift> {
    let c = chars(line.trim());
    for at in 0..c.len() {
        if c[at] != '+' && c[at] != '-' {
            continue;
        }
        let from = slice(&c, 0, at);
        if from.is_empty() {
            continue;
        }
        let right = slice(&c, at + 1, c.len());
        let Some((list, left)) = amounts(&right) else {
            continue;
        };
        let Some(zone) = zone_tail(&left, IN_TO, false) else {
            continue;
        };
        return Some(Shift {
            from,
            minus: c[at] == '-',
            amounts: list,
            zone,
        });
    }
    None
}

pub fn date_shift(line: &str) -> Option<DateShift> {
    let c = chars(line.trim());
    for at in 0..c.len() {
        if c[at] != '+' && c[at] != '-' {
            continue;
        }
        let from = slice(&c, 0, at);
        if from.is_empty() {
            continue;
        }
        let Some((n, unit)) = cal_amount(&slice(&c, at + 1, c.len())) else {
            continue;
        };
        return Some(DateShift {
            from,
            minus: c[at] == '-',
            n,
            unit,
        });
    }
    None
}

pub fn date_diff(line: &str) -> Option<DateDiff> {
    let c = chars(line.trim());
    for at in 0..c.len() {
        if c[at] != '-' {
            continue;
        }
        if at == 0 || at + 1 >= c.len() {
            continue;
        }
        if !(c[at - 1].is_whitespace() && c[at + 1].is_whitespace()) {
            continue;
        }
        let a = slice(&c, 0, at);
        let b = slice(&c, at + 1, c.len());
        if a.is_empty() || b.is_empty() {
            continue;
        }
        return Some(DateDiff { a, b });
    }
    None
}

pub fn until(line: &str) -> Option<Until> {
    let c = chars(line.trim());
    let found = runs(&c);
    let mut at = 0;
    if let Some((taken, _, _)) = match_phrase(&found, at, HEADINGS) {
        at += taken;
    }
    // The unit is optional, but only counts when a relation follows it right
    // away; otherwise the relation is required at the original index, the way
    // a greedy optional group backtracks.
    let mut unit = None;
    let mut relation = None;
    if let Some((unit_taken, _, unit_text)) = match_phrase(&found, at, SPAN_UNITS)
        && let Some((_, rel_end, rel_word)) = match_phrase(&found, at + unit_taken, RELATIONS)
    {
        unit = Some(unit_text);
        relation = Some((rel_end, rel_word));
    }
    let (rel_end, rel_word) = match relation {
        Some(pair) => pair,
        None => {
            let (_, end, word) = match_phrase(&found, at, RELATIONS)?;
            (end, word)
        }
    };
    let date = strip_query(&slice(&c, rel_end, c.len()));
    if date.is_empty() {
        return None;
    }
    Some(Until {
        unit,
        until: rel_word != "since",
        date,
    })
}

fn strip_query(text: &str) -> String {
    match text.strip_suffix('?') {
        Some(short) => short.trim().to_string(),
        None => text.to_string(),
    }
}

pub fn between(line: &str) -> Option<Between> {
    let c = chars(line.trim());
    let found = runs(&c);
    let mut at = 0;
    if let Some((taken, _, _)) = match_phrase(&found, at, HOW_MANY) {
        at += taken;
    }
    let (taken, _, unit) = match_phrase(&found, at, BETWEEN_UNITS)?;
    at += taken;
    let (_, pair_end, _) = match_phrase(&found, at, PAIRS)?;
    let right = slice(&c, pair_end, c.len());
    let (a, _, b) = find_split(&right, JOINS)?;
    let b = strip_query(&b);
    if a.is_empty() || b.is_empty() {
        return None;
    }
    Some(Between { unit, a, b })
}

pub fn month_count(line: &str) -> Option<MonthCount> {
    let mut cur = Cur::new(line.trim());
    let save = cur.mark();
    if phrase_at(&mut cur, HOW_MANY).is_none() {
        cur.back(save);
    }
    let unit = phrase_at(&mut cur, COUNT_UNITS)?;
    cur.ws();
    if cur.word().as_deref() != Some("in") || !cur.peek().is_some_and(|c| c.is_whitespace()) {
        return None;
    }
    cur.ws();
    let name = cur.letters()?;
    let month = month_of(&name)?;
    cur.eat('.');
    let save = cur.mark();
    if cur.peek().is_some_and(|c| c.is_whitespace()) {
        cur.ws();
        match cur.digits() {
            Some(digits) if digits.len() == 4 => {
                let year = digits.parse().ok()?;
                return finish_month_count(&mut cur, unit, month, Some(year));
            }
            _ => cur.back(save),
        }
    }
    finish_month_count(&mut cur, unit, month, None)
}

fn finish_month_count(
    cur: &mut Cur,
    unit: String,
    month: u32,
    year: Option<u32>,
) -> Option<MonthCount> {
    cur.eat('?');
    if !cur.done() {
        return None;
    }
    Some(MonthCount { unit, month, year })
}

fn phrase_at(cur: &mut Cur, table: &[&str]) -> Option<String> {
    let mut best: Option<(usize, String, usize)> = None;
    for phrase in table {
        let save = cur.mark();
        let parts: Vec<&str> = phrase.split_whitespace().collect();
        let mut ok = true;
        for part in &parts {
            cur.ws();
            if cur.word().as_deref() != Some(*part) || !cur.bounded() {
                ok = false;
                break;
            }
        }
        if ok && best.as_ref().is_none_or(|(len, _, _)| parts.len() > *len) {
            best = Some((parts.len(), parts.join(" "), cur.mark()));
        }
        cur.back(save);
    }
    let (_, text, at) = best?;
    cur.back(at);
    Some(text)
}

fn day_and_ordinal(cur: &mut Cur) -> Option<u32> {
    let start = cur.mark();
    let digits = cur.digits()?;
    if digits.len() > 2 {
        cur.back(start);
        return None;
    }
    let day: u32 = digits.parse().ok()?;
    for suffix in ["st", "nd", "rd", "th"] {
        let at = cur.mark();
        if cur.letters().as_deref() == Some(suffix) && cur.bounded() {
            return Some(day);
        }
        cur.back(at);
    }
    if !cur.bounded() {
        cur.back(start);
        return None;
    }
    Some(day)
}

fn optional_year(cur: &mut Cur) -> Option<Option<u32>> {
    let save = cur.mark();
    if !cur.peek().is_some_and(|c| c.is_whitespace() || c == ',') {
        return Some(None);
    }
    cur.eat(',');
    if !cur.peek().is_some_and(|c| c.is_whitespace()) {
        cur.back(save);
        return Some(None);
    }
    cur.ws();
    let start = cur.mark();
    let Some(digits) = cur.digits() else {
        cur.back(save);
        return Some(None);
    };
    if digits.len() != 4 || !cur.bounded() {
        cur.back(start);
        return Some(None);
    }
    Some(Some(digits.parse().ok()?))
}

fn named_month(cur: &mut Cur) -> Option<u32> {
    let name = cur.letters()?;
    let month = month_of(&name)?;
    cur.eat('.');
    Some(month)
}

fn iso_at(c: &[char], at: usize) -> Option<(usize, u32, u32, u32)> {
    let mut cur = Cur::at(c, at);
    let digits = cur.digits()?;
    if digits.len() != 4 {
        return None;
    }
    let year: u32 = digits.parse().ok()?;
    if !cur.eat('-') {
        return None;
    }
    let digits = cur.digits()?;
    if digits.len() != 2 {
        return None;
    }
    let month = digits.parse::<u32>().ok()?;
    if !cur.eat('-') {
        return None;
    }
    let digits = cur.digits()?;
    if digits.len() != 2 || !cur.bounded() {
        return None;
    }
    let day = digits.parse::<u32>().ok()?;
    Some((cur.at, year, month, day))
}

pub fn iso_dates(text: &str) -> Vec<(i32, u32, u32)> {
    let c = chars(text);
    let mut out = Vec::new();
    let mut at = 0;
    while at < c.len() {
        if c[at].is_ascii_digit()
            && (at == 0 || !wordish(c[at - 1]))
            && let Some((end, year, month, day)) = iso_at(&c, at)
        {
            out.push((year as i32, month, day));
            at = end;
            continue;
        }
        at += 1;
    }
    out
}

/// One alternative of an ordered choice: try it from `start`, keep it only when
/// it ends on a word boundary, and rewind the cursor when it does not stand.
fn try_at(
    c: &[char],
    cur: &mut Cur,
    start: usize,
    attempt: impl FnOnce(&mut Cur) -> Option<(usize, DatePhrase)>,
) -> Option<(usize, DatePhrase)> {
    match attempt(cur) {
        Some((end, phrase)) if end == c.len() || !wordish(c[end]) => Some((end, phrase)),
        _ => {
            cur.back(start);
            None
        }
    }
}

fn date_at(c: &[char], at: usize) -> Option<(usize, DatePhrase)> {
    let mut cur = Cur::at(c, at);
    let save = cur.mark();
    if cur.word().is_some_and(|w| w == "on") && cur.peek().is_some_and(|c| c.is_whitespace()) {
        cur.ws();
    } else {
        cur.back(save);
    }
    let start = cur.mark();
    for attempt in [relative_at, weekday_at, month_first, day_first] {
        if let Some(found) = try_at(c, &mut cur, start, attempt) {
            return Some(found);
        }
    }
    if let Some((end, year, month, day)) = iso_at(c, start) {
        return Some((
            end,
            DatePhrase::MonthDay {
                month,
                day,
                year: Some(year),
            },
        ));
    }
    None
}

fn relative_at(cur: &mut Cur) -> Option<(usize, DatePhrase)> {
    let word = cur.word()?;
    let offset = match word.as_str() {
        "today" => 0,
        "tomorrow" => 1,
        "yesterday" => -1,
        _ => return None,
    };
    Some((cur.at, DatePhrase::Offset(offset)))
}

fn weekday_at(cur: &mut Cur) -> Option<(usize, DatePhrase)> {
    let save = cur.mark();
    let mut next = false;
    if cur.word().is_some_and(|w| w == "next") && cur.peek().is_some_and(|c| c.is_whitespace()) {
        cur.ws();
        next = true;
    } else {
        cur.back(save);
    }
    let day = weekday_of(&cur.word()?)?;
    Some((cur.at, DatePhrase::Weekday { day, next }))
}

fn month_first(cur: &mut Cur) -> Option<(usize, DatePhrase)> {
    let month = named_month(cur)?;
    if !cur.peek().is_some_and(|c| c.is_whitespace()) {
        return None;
    }
    cur.ws();
    let day = day_and_ordinal(cur)?;
    let stop = cur.mark();
    let year = optional_year(cur)?;
    if year.is_none() {
        cur.back(stop);
    }
    Some((cur.at, DatePhrase::MonthDay { month, day, year }))
}

fn day_first(cur: &mut Cur) -> Option<(usize, DatePhrase)> {
    let day = day_and_ordinal(cur)?;
    if !cur.peek().is_some_and(|c| c.is_whitespace()) {
        return None;
    }
    cur.ws();
    let month = named_month(cur)?;
    let stop = cur.mark();
    let year = optional_year(cur)?;
    if year.is_none() {
        cur.back(stop);
    }
    Some((cur.at, DatePhrase::MonthDay { month, day, year }))
}

pub fn take_date(text: &str) -> (String, Option<DatePhrase>) {
    let text = text.trim();
    let c = chars(text);
    let mut at = 0;
    while at < c.len() {
        if wordish(c[at])
            && (at == 0 || !wordish(c[at - 1]))
            && let Some((end, phrase)) = date_at(&c, at)
            && end > at
        {
            let mut out = String::new();
            out.extend(c[..at].iter());
            out.push(' ');
            out.extend(c[end..].iter());
            return (collapse(&out), Some(phrase));
        }
        at += 1;
    }
    (text.to_string(), None)
}

pub fn slash(text: &str) -> Option<Slash> {
    let mut cur = Cur::new(text.trim());
    let a = digits_upto(&mut cur, 2)?;
    if !cur.eat('/') {
        return None;
    }
    let b = digits_upto(&mut cur, 2)?;
    let mut year = None;
    if cur.eat('/') {
        let save = cur.mark();
        let digits = cur.digits()?;
        if digits.len() != 2 && digits.len() != 4 {
            cur.back(save);
            return None;
        }
        year = Some(digits);
    }
    if !cur.done() {
        return None;
    }
    Some(Slash { a, b, year })
}

fn meridiem_at(cur: &mut Cur) -> Option<String> {
    for word in ["a.m.", "p.m.", "am", "pm"] {
        let save = cur.mark();
        let want: Vec<char> = word.chars().collect();
        let mut ok = true;
        for ch in &want {
            if cur.peek() == Some(*ch) {
                cur.at += 1;
            } else {
                ok = false;
                break;
            }
        }
        if ok {
            return Some(word.to_string());
        }
        cur.back(save);
    }
    None
}

fn clock_at(c: &[char], at: usize) -> Option<Clock> {
    let mut cur = Cur::at(c, at);
    let save = cur.mark();
    if let Some(word) = cur.word()
        && matches!(word.as_str(), "noon" | "midnight" | "now")
    {
        cur.ws();
        return Some(Clock {
            word: Some(word),
            hour: None,
            minute: None,
            meridiem: None,
            rest: cur.rest(),
        });
    }
    cur.back(save);
    let digits = cur.digits()?;
    if digits.len() > 2 {
        return None;
    }
    let hour = digits.parse().ok()?;
    let mut minute = None;
    if cur.eat(':') {
        let tail = cur.digits()?;
        if tail.len() != 2 {
            return None;
        }
        minute = Some(tail.parse().ok()?);
    }
    cur.ws();
    let meridiem = meridiem_at(&mut cur);
    cur.ws();
    Some(Clock {
        word: None,
        hour: Some(hour),
        minute,
        meridiem,
        rest: cur.rest(),
    })
}

pub fn clock(text: &str) -> Option<Clock> {
    let c = chars(text);
    let mut at = 0;
    loop {
        while at < c.len() && c[at].is_whitespace() {
            at += 1;
        }
        if at >= c.len() {
            return None;
        }
        if let Some(found) = clock_at(&c, at) {
            return Some(found);
        }
        while at < c.len() && !c[at].is_whitespace() {
            at += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_parse_as_amounts() {
        let (list, left) = amounts("1 hour 30 min").unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].unit, SpanUnit::Hour);
        assert_eq!(list[1].unit, SpanUnit::Minute);
        assert!(left.is_empty());
        assert_eq!(amounts("45 minutes in London").unwrap().1, "in London");
        assert!(amounts("3 m").is_none());
    }

    #[test]
    fn zone_and_conversion_shapes() {
        assert_eq!(zone_now("time in Tokyo").unwrap().zone, "Tokyo");
        assert!(zone_now("what time is it in Rome").is_none());
        assert_eq!(zone_now("now in India?").unwrap().zone, "India");
        assert_eq!(convert("3pm PST in IST").unwrap().from, "3pm PST");
        assert_eq!(convert("3pm PST in IST").unwrap().zone, "IST");
        assert!(convert("into 5").is_none());
        assert!(convert("2 hours into minutes").is_none());
    }

    #[test]
    fn relative_shapes() {
        let now = from_now("in 3 days in UTC").unwrap();
        assert_eq!(now.amounts.len(), 1);
        assert_eq!(now.zone.as_deref(), Some("UTC"));
        assert_eq!(
            from_now("3 hours from now in UTC").unwrap().zone.as_deref(),
            Some("UTC")
        );
        assert!(from_now("in 3 m").is_none());
        let moved = shift("9:30am PST + 2 hours in London").unwrap();
        assert_eq!(moved.from, "9:30am PST");
        assert!(!moved.minus);
        assert_eq!(moved.zone.as_deref(), Some("London"));
        assert!(shift("3 + 45 min").is_some());
    }

    #[test]
    fn calendar_shapes() {
        let shift = date_shift("last friday + 1 week").unwrap();
        assert_eq!(shift.from, "last friday");
        assert_eq!(shift.n, 1);
        assert_eq!(shift.unit, Unit::Week);
        assert_eq!(
            date_diff("Dec 25 2026 - last Friday").unwrap().a,
            "Dec 25 2026"
        );
        assert!(basic("now + 90 minutes").unwrap().offset.is_some());
        assert!(basic("now + 5 h").is_none());
    }

    #[test]
    fn counting_shapes() {
        let spanned = until("how many work days until Oct 27").unwrap();
        assert_eq!(spanned.unit.as_deref(), Some("work days"));
        assert_eq!(spanned.date, "Oct 27");
        assert!(until("until Christmas?").unwrap().unit.is_none());
        let pair = between("days from Dec 20 to Jan 5").unwrap();
        assert_eq!(pair.a, "Dec 20");
        assert_eq!(pair.b, "Jan 5");
        let month = month_count("days in February 2028").unwrap();
        assert_eq!(month.month, 2);
        assert_eq!(month.year, Some(2028));
        assert_eq!(month_count("workdays in November").unwrap().year, None);
    }

    #[test]
    fn dates_are_extracted_without_regex() {
        let (rest, phrase) = take_date("Launch 2026-11-02 10am ET");
        assert_eq!(
            phrase,
            Some(DatePhrase::MonthDay {
                month: 11,
                day: 2,
                year: Some(2026)
            })
        );
        assert_eq!(rest, "Launch 10am ET");
        assert_eq!(
            take_date("on next Friday"),
            (
                String::new(),
                Some(DatePhrase::Weekday { day: 4, next: true })
            )
        );
        assert_eq!(
            take_date("1 December 2026").1,
            Some(DatePhrase::MonthDay {
                month: 12,
                day: 1,
                year: Some(2026)
            })
        );
        let (rest, phrase) = take_date("Dec 1 3pm PST");
        assert_eq!(&rest, "3pm PST");
        assert_eq!(
            phrase,
            Some(DatePhrase::MonthDay {
                month: 12,
                day: 1,
                year: None
            })
        );
        assert_eq!(take_date("Oct 2026").1, None);
        assert_eq!(iso_dates("a 2026-02-30 b 2026-10-16").len(), 2);
        assert_eq!(slash("15/1/26").unwrap().b, 1);
        assert_eq!(slash("12/25/26").unwrap().year.as_deref(), Some("26"));
        assert!(slash("1/2/345").is_none());
    }

    #[test]
    fn clocks_scan_forward() {
        assert_eq!(clock("Standup 6:12pm IST").unwrap().hour, Some(6));
        assert_eq!(
            clock("Standup 6:12pm IST").unwrap().meridiem.as_deref(),
            Some("pm")
        );
        assert_eq!(clock("Standup 6:12pm IST").unwrap().rest, "IST");
        assert_eq!(
            clock("noon UTC in tokyo").unwrap().word.as_deref(),
            Some("noon")
        );
        assert_eq!(clock("3 pm").unwrap().rest, "");
        assert!(clock("Meeting").is_none());
        assert!(clock("Meeting notes").is_none());
    }
}
