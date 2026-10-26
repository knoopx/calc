use std::collections::HashMap;
use std::sync::LazyLock;

use crate::engine::peg::time as phrase;
use crate::engine::peg::time::{Amount, Base, SpanUnit, Unit};
use chrono::{
    DateTime, Datelike, Days, Duration, Local, Months, NaiveDate, NaiveDateTime, NaiveTime,
    TimeZone, Timelike, Utc, Weekday,
};
use chrono_tz::Tz;

const HOLIDAYS: &[(&str, u32, u32)] = &[
    ("christmas eve", 12, 24),
    ("christmas", 12, 25),
    ("xmas", 12, 25),
    ("new year's eve", 12, 31),
    ("new years eve", 12, 31),
    ("new year's day", 1, 1),
    ("new year's", 1, 1),
    ("new years", 1, 1),
    ("new year", 1, 1),
    ("halloween", 10, 31),
    ("valentine's day", 2, 14),
    ("valentines day", 2, 14),
    ("valentine's", 2, 14),
];

const ABBREVIATIONS: &[(&str, &str)] = &[
    ("PST", "America/Los_Angeles"),
    ("PDT", "America/Los_Angeles"),
    ("PT", "America/Los_Angeles"),
    ("MST", "America/Denver"),
    ("MDT", "America/Denver"),
    ("MT", "America/Denver"),
    ("CST", "America/Chicago"),
    ("CDT", "America/Chicago"),
    ("CT", "America/Chicago"),
    ("EST", "America/New_York"),
    ("EDT", "America/New_York"),
    ("ET", "America/New_York"),
    ("AKST", "America/Anchorage"),
    ("HST", "Pacific/Honolulu"),
    ("UTC", "UTC"),
    ("GMT", "Etc/GMT"),
    ("BST", "Europe/London"),
    ("WET", "Europe/Lisbon"),
    ("CET", "Europe/Paris"),
    ("CEST", "Europe/Paris"),
    ("EET", "Europe/Athens"),
    ("MSK", "Europe/Moscow"),
    ("IST", "Asia/Kolkata"),
    ("PKT", "Asia/Karachi"),
    ("NPT", "Asia/Kathmandu"),
    ("GST", "Asia/Dubai"),
    ("SGT", "Asia/Singapore"),
    ("HKT", "Asia/Hong_Kong"),
    ("JST", "Asia/Tokyo"),
    ("KST", "Asia/Seoul"),
    ("AEST", "Australia/Sydney"),
    ("AEDT", "Australia/Sydney"),
    ("NZST", "Pacific/Auckland"),
    ("NZDT", "Pacific/Auckland"),
];

const PLACES: &[(&str, &str)] = &[
    ("india", "Asia/Kolkata"),
    ("mumbai", "Asia/Kolkata"),
    ("delhi", "Asia/Kolkata"),
    ("new delhi", "Asia/Kolkata"),
    ("bangalore", "Asia/Kolkata"),
    ("bengaluru", "Asia/Kolkata"),
    ("chennai", "Asia/Kolkata"),
    ("hyderabad", "Asia/Kolkata"),
    ("pune", "Asia/Kolkata"),
    ("kolkata", "Asia/Kolkata"),
    ("sf", "America/Los_Angeles"),
    ("san francisco", "America/Los_Angeles"),
    ("seattle", "America/Los_Angeles"),
    ("california", "America/Los_Angeles"),
    ("nyc", "America/New_York"),
    ("boston", "America/New_York"),
    ("miami", "America/New_York"),
    ("washington", "America/New_York"),
    ("austin", "America/Chicago"),
    ("dallas", "America/Chicago"),
    ("houston", "America/Chicago"),
    ("uk", "Europe/London"),
    ("england", "Europe/London"),
    ("germany", "Europe/Berlin"),
    ("france", "Europe/Paris"),
    ("japan", "Asia/Tokyo"),
    ("china", "Asia/Shanghai"),
    ("beijing", "Asia/Shanghai"),
    ("uae", "Asia/Dubai"),
    ("dubai", "Asia/Dubai"),
    ("australia", "Australia/Sydney"),
    ("utc", "UTC"),
    ("local", "local"),
    ("here", "local"),
];

static ZONES: LazyLock<HashMap<String, Tz>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    for tz in chrono_tz::TZ_VARIANTS {
        let name = tz.name();
        map.insert(name.to_ascii_lowercase(), tz);
        if let Some(city) = name.rsplit('/').next() {
            map.entry(city.replace('_', " ").to_ascii_lowercase())
                .or_insert(tz);
        }
    }
    map
});

#[derive(Clone, Copy)]
enum Zone {
    Local,
    Tz(Tz),
}

impl Zone {
    fn at(self, utc: DateTime<Utc>) -> (NaiveDate, String) {
        match self {
            Zone::Local => {
                let t = utc.with_timezone(&Local);
                (
                    t.date_naive(),
                    format!("{} {}", clock(t.time()), t.format("%Z")),
                )
            }
            Zone::Tz(tz) => {
                let t = utc.with_timezone(&tz);
                (
                    t.date_naive(),
                    format!(
                        "{} {}",
                        clock(t.time()),
                        label(tz, &t.format("%Z").to_string())
                    ),
                )
            }
        }
    }

    fn wall(self, utc: DateTime<Utc>) -> NaiveDateTime {
        match self {
            Zone::Local => utc.with_timezone(&Local).naive_local(),
            Zone::Tz(tz) => utc.with_timezone(&tz).naive_local(),
        }
    }

    fn to_utc(self, date: NaiveDate, time: NaiveTime) -> Option<DateTime<Utc>> {
        let naive = date.and_time(time);
        match self {
            Zone::Local => Local
                .from_local_datetime(&naive)
                .earliest()
                .map(|t| t.to_utc()),
            Zone::Tz(tz) => tz
                .from_local_datetime(&naive)
                .earliest()
                .map(|t| t.to_utc()),
        }
    }
}

fn zone(name: &str) -> Option<Zone> {
    let name = name.trim().trim_end_matches(" time");
    let upper = name.to_ascii_uppercase();
    let lower = name.to_ascii_lowercase();
    let iana = ABBREVIATIONS
        .iter()
        .find(|(a, _)| *a == upper)
        .or_else(|| PLACES.iter().find(|(p, _)| *p == lower))
        .map(|(_, z)| *z);
    match iana {
        Some("local") => Some(Zone::Local),
        Some(z) => z.parse().ok().map(Zone::Tz),
        None => ZONES.get(&lower).copied().map(Zone::Tz),
    }
}

fn label(tz: Tz, abbr: &str) -> String {
    if !abbr.starts_with(['+', '-']) {
        return abbr.to_string();
    }
    ABBREVIATIONS
        .iter()
        .find(|(_, z)| *z == tz.name())
        .map_or_else(|| format!("UTC{abbr}"), |(a, _)| (*a).to_string())
}

pub fn eval(line: &str) -> Option<String> {
    let now = Utc::now();
    let today = Local::now().date_naive();
    eval_span(line, today)
        .or_else(|| eval_date(line, Local::now()))
        .or_else(|| eval_lone(line, today))
        .or_else(|| eval_date_math(line, today))
        .or_else(|| eval_relative(line, now))
        .or_else(|| eval_zone(line, now))
}

pub fn note(line: &str) -> Option<&'static str> {
    let line = line.trim();
    let dates: Vec<String> = if let Some(c) = phrase::until(line) {
        vec![c.date]
    } else {
        let c = phrase::between(line)?;
        vec![c.a, c.b]
    };
    dates
        .iter()
        .any(|d| matches!(slash_date(d), Some(None)))
        .then_some("Write the month: Feb 3 or 3 Feb")
}

fn slash_date(text: &str) -> Option<Option<String>> {
    let c = phrase::slash(text)?;
    let (a, b) = (c.a, c.b);
    let (month, day) = match (a <= 12, b <= 12) {
        _ if a == b => (a, b),
        (true, false) => (a, b),
        (false, true) => (b, a),
        (true, true) => return Some(None),
        (false, false) => return None,
    };
    let name = *[
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .get(month.checked_sub(1)? as usize)?;
    let year = match c.year.as_deref() {
        Some(y) if y.len() == 2 => format!(" 20{y}"),
        Some(y) => format!(" {y}"),
        None => String::new(),
    };
    Some(Some(format!("{name} {day}{year}")))
}

fn span_date(text: &str, today: NaiveDate, ahead: bool) -> Option<NaiveDate> {
    match slash_date(text) {
        Some(Some(named)) => lone_date(&named, today, ahead),
        Some(None) => None,
        None => lone_date(text, today, ahead),
    }
}

fn eval_lone(line: &str, today: NaiveDate) -> Option<String> {
    let text = line.trim();
    let lower = text.to_ascii_lowercase();
    let (text, lead) = match ["on ", "this ", "last "]
        .iter()
        .find(|p| lower.starts_with(*p))
    {
        Some(p) => (text[p.len()..].trim(), Some(p.trim())),
        None => (text, None),
    };
    let (rest, spec) = take_date(text);
    if !rest.is_empty() {
        return None;
    }
    let date = match spec? {
        DateSpec::Offset(_) => return None,
        DateSpec::Weekday { next, .. } => match lead {
            Some(_) if !next => led_weekday(line, today)?,
            None if next => lone_date(text, today, true)?,
            _ => return None,
        },
        DateSpec::MonthDay { .. } if lead.is_none() || lead == Some("on") => {
            this_years(text, today)?
        }
        DateSpec::MonthDay { .. } => return None,
    };
    Some(date.format("%A, %-d %B %Y").to_string())
}

fn led_weekday(text: &str, today: NaiveDate) -> Option<NaiveDate> {
    let text = text.trim();
    let lower = text.to_ascii_lowercase();
    let (lead, ahead) = [("last ", false), ("this ", true), ("on ", true)]
        .into_iter()
        .find(|(p, _)| lower.starts_with(p))?;
    let day = text[lead.len()..].trim();
    match take_date(day) {
        (rest, Some(DateSpec::Weekday { next: false, .. })) if rest.is_empty() => {
            lone_date(day, today, ahead)
        }
        _ => None,
    }
}

pub fn impossible_date(line: &str) -> bool {
    let line = line.trim();
    let iso = phrase::iso_dates(line)
        .into_iter()
        .any(|(y, m, d)| NaiveDate::from_ymd_opt(y, m, d).is_none());
    let mut dates = vec![line.to_string()];
    if let Some(c) = phrase::date_shift(line) {
        dates.push(c.from);
    }
    if let Some(c) = phrase::date_diff(line) {
        dates.extend([c.a, c.b]);
    }
    if let Some(c) = phrase::until(line) {
        dates.push(c.date);
    }
    if let Some(c) = phrase::between(line) {
        dates.extend([c.a, c.b]);
    }
    iso || dates.iter().any(|d| {
        let d = d.trim();
        let d = d.strip_prefix("on ").unwrap_or(d);
        match take_date(d) {
            (rest, Some(DateSpec::MonthDay { month, day, year })) if rest.is_empty() => {
                let year = year.unwrap_or_else(|| Local::now().year());
                NaiveDate::from_ymd_opt(year, month, day).is_none()
            }
            _ => false,
        }
    })
}

pub fn completion_words() -> impl Iterator<Item = &'static str> {
    ABBREVIATIONS.iter().map(|(a, _)| *a)
}

pub fn completion_meaning(word: &str) -> Option<&'static str> {
    let upper = word.to_ascii_uppercase();
    ABBREVIATIONS
        .iter()
        .find(|(a, _)| *a == upper)
        .map(|(_, zone)| *zone)
}

fn eval_zone(line: &str, now: DateTime<Utc>) -> Option<String> {
    if let Some(c) = phrase::zone_now(line) {
        return Some(zone(&c.zone)?.at(now).1);
    }

    let c = phrase::convert(line)?;
    let target = zone(&c.zone)?;
    let (instant, source, date) = instant(&c.from, now)?;
    let (from_date, _) = source.at(instant);
    let (to_date, shown) = target.at(instant);
    if date.is_some() {
        return Some(format!("{shown}, {}", day_label(to_date, source.at(now).0)));
    }
    Some(with_day_change(shown, from_date, to_date))
}

fn with_day_change(shown: String, from: NaiveDate, to: NaiveDate) -> String {
    match (to - from).num_days() {
        1 => format!("{shown} (next day)"),
        -1 => format!("{shown} (prev day)"),
        _ => shown,
    }
}

fn instant(lhs: &str, now: DateTime<Utc>) -> Option<(DateTime<Utc>, Zone, Option<DateSpec>)> {
    let (lhs, date) = take_date(lhs.trim());

    let t = phrase::clock(&lhs)?;
    let rest = t.rest.clone();
    let source = if rest.is_empty() {
        Zone::Local
    } else {
        zone(&rest)?
    };

    if t.word.as_deref().is_some_and(|w| w == "now") {
        return Some((now, source, date));
    }
    let time = match t.word.as_deref() {
        Some("noon") => NaiveTime::from_hms_opt(12, 0, 0)?,
        Some(_) => NaiveTime::from_hms_opt(0, 0, 0)?,
        None => {
            let mut hour: u32 = t.hour?;
            let minute: u32 = t.minute.unwrap_or(0);
            let meridiem = t.meridiem.clone();

            if meridiem.is_none() && t.minute.is_none() && rest.is_empty() {
                return None;
            }
            match meridiem.as_deref().map(|m| m.starts_with('p')) {
                Some(true) if hour < 12 => hour += 12,
                Some(false) if hour == 12 => hour = 0,
                _ => {}
            }
            NaiveTime::from_hms_opt(hour, minute, 0)?
        }
    };
    let today = source.at(now).0;
    let day = match date {
        Some(d) => d.resolve(today)?,
        None => today,
    };
    Some((source.to_utc(day, time)?, source, date))
}

struct Span {
    days: i64,
    rest: Duration,
}

fn duration(amounts: &[Amount]) -> Option<Span> {
    let (mut days, mut seconds) = (0i64, 0.0);
    for amount in amounts {
        let n = amount.n;
        let per_day = match amount.unit {
            SpanUnit::Week => 7.0,
            SpanUnit::Day => 1.0,
            _ => 0.0,
        };
        let whole = (n * per_day).trunc();
        if per_day > 0.0 && whole.abs() < 1e9 {
            days += whole as i64;
            seconds += (n * per_day - whole) * 86400.0;
        } else {
            seconds += n * match amount.unit {
                SpanUnit::Hour => 3600.0,
                SpanUnit::Week | SpanUnit::Day => per_day * 86400.0,
                SpanUnit::Minute => 60.0,
            };
        }
    }

    if !seconds.is_finite() || seconds.abs() > 1e13 || days.abs() > 100_000_000 {
        return None;
    }
    Some(Span {
        days,
        rest: Duration::try_seconds(seconds.round() as i64)?,
    })
}

fn shift(start: DateTime<Utc>, zone: Zone, span: &Span, back: bool) -> Option<DateTime<Utc>> {
    let wall = zone.wall(start);
    let days = Days::new(span.days.unsigned_abs());
    let wall = if back == (span.days >= 0) {
        wall.checked_sub_days(days)?
    } else {
        wall.checked_add_days(days)?
    };

    let moved = zone
        .to_utc(wall.date(), wall.time())
        .or_else(|| zone.to_utc(wall.date(), wall.time() + Duration::hours(1)))?;
    if back {
        moved.checked_sub_signed(span.rest)
    } else {
        moved.checked_add_signed(span.rest)
    }
}

fn day_label(day: NaiveDate, today: NaiveDate) -> String {
    if day.year() == today.year() {
        day.format("%a %-d %b").to_string()
    } else {
        day.format("%a %-d %b %Y").to_string()
    }
}

fn eval_relative(line: &str, now: DateTime<Utc>) -> Option<String> {
    if let Some(c) = phrase::from_now(line) {
        let span = duration(&c.amounts)?;
        let target = match c.zone.as_deref() {
            Some(z) => zone(z)?,
            None => Zone::Local,
        };
        let then = shift(now, target, &span, false)?;
        let (today, _) = target.at(now);
        let (day, shown) = target.at(then);
        let shown = match target {
            Zone::Local => clock(then.with_timezone(&Local).time()),
            Zone::Tz(_) => shown,
        };
        return Some(if day == today {
            shown
        } else {
            format!("{shown}, {}", day_label(day, today))
        });
    }

    let c = phrase::shift(line)?;
    let (start, source, _) = instant(&c.from, now)?;
    let span = duration(&c.amounts)?;
    let then = shift(start, source, &span, c.minus)?;
    let target = match c.zone.as_deref() {
        Some(z) => zone(z)?,
        None => source,
    };
    let (from_date, _) = source.at(start);
    let (to_date, shown) = target.at(then);

    if (to_date - from_date).num_days().abs() > 1 {
        let shown = match (c.zone.as_ref(), target) {
            (None, Zone::Local) => clock(then.with_timezone(&Local).time()),
            _ => shown,
        };
        return Some(format!("{shown}, {}", day_label(to_date, from_date)));
    }

    let shown = match (c.zone.as_ref(), target) {
        (None, Zone::Local) => clock(then.with_timezone(&Local).time()),
        _ => shown,
    };
    Some(with_day_change(shown, from_date, to_date))
}

#[derive(Clone, Copy)]
enum DateSpec {
    Offset(i64),
    Weekday {
        day: Weekday,
        next: bool,
    },
    MonthDay {
        month: u32,
        day: u32,
        year: Option<i32>,
    },
}

impl DateSpec {
    fn resolve(self, today: NaiveDate) -> Option<NaiveDate> {
        match self {
            DateSpec::Offset(n) => today.checked_add_signed(Duration::days(n)),
            DateSpec::Weekday { day, next } => {
                let ahead = (7 + day.num_days_from_monday() as i64
                    - today.weekday().num_days_from_monday() as i64)
                    % 7;

                let ahead = if next && ahead == 0 { 7 } else { ahead };
                Some(today + Duration::days(ahead))
            }
            DateSpec::MonthDay {
                month,
                day,
                year: Some(y),
            } => NaiveDate::from_ymd_opt(y, month, day),
            DateSpec::MonthDay {
                month,
                day,
                year: None,
            } => {
                let this = NaiveDate::from_ymd_opt(today.year(), month, day)?;
                if this < today - Duration::days(60) {
                    NaiveDate::from_ymd_opt(today.year() + 1, month, day)
                } else {
                    Some(this)
                }
            }
        }
    }
}

fn take_date(lhs: &str) -> (String, Option<DateSpec>) {
    let (rest, found) = phrase::take_date(lhs);
    (rest, found.map(resolved))
}

fn resolved(found: phrase::DatePhrase) -> DateSpec {
    match found {
        phrase::DatePhrase::Offset(days) => DateSpec::Offset(days),
        phrase::DatePhrase::Weekday { day, next } => DateSpec::Weekday {
            day: match day {
                0 => Weekday::Mon,
                1 => Weekday::Tue,
                2 => Weekday::Wed,
                3 => Weekday::Thu,
                4 => Weekday::Fri,
                5 => Weekday::Sat,
                _ => Weekday::Sun,
            },
            next,
        },
        phrase::DatePhrase::MonthDay { month, day, year } => DateSpec::MonthDay {
            month,
            day,
            year: year.map(|y| y as i32),
        },
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Count {
    Days,
    Weeks,
    Months,
    Workdays,
}

fn count_unit(word: Option<&str>) -> Count {
    match word.map(|w| w.to_ascii_lowercase()) {
        Some(w) if w.starts_with("week") && w != "weekdays" => Count::Weeks,
        Some(w) if w.starts_with("month") => Count::Months,
        Some(w) if w.contains("day") && w != "days" => Count::Workdays,
        _ => Count::Days,
    }
}

fn lone_date(text: &str, today: NaiveDate, ahead: bool) -> Option<NaiveDate> {
    let text = text.trim().trim_start_matches("the ").trim();
    let lower = text.to_ascii_lowercase();
    let (month, day, year) = if let Some(&(_, m, d)) = HOLIDAYS.iter().find(|(n, ..)| *n == lower) {
        (m, d, None)
    } else {
        let (rest, spec) = take_date(text);
        let spec = spec.filter(|_| rest.is_empty())?;
        match spec {
            DateSpec::MonthDay { month, day, year } => (month, day, year),
            DateSpec::Weekday { day, next } if !ahead => {
                let back = (7 + today.weekday().num_days_from_monday() as i64
                    - day.num_days_from_monday() as i64)
                    % 7;
                let back = if next || back == 0 { back + 7 } else { back };
                return today.checked_sub_signed(Duration::days(back));
            }
            other => return other.resolve(today),
        }
    };
    if let Some(y) = year {
        return NaiveDate::from_ymd_opt(y, month, day);
    }
    let this = NaiveDate::from_ymd_opt(today.year(), month, day)?;
    let year = match (ahead, this < today, this > today) {
        (true, true, _) => today.year() + 1,
        (false, _, true) => today.year() - 1,
        _ => today.year(),
    };
    NaiveDate::from_ymd_opt(year, month, day)
}

fn workdays(from: NaiveDate, to: NaiveDate) -> i64 {
    let (a, b) = if from <= to { (from, to) } else { (to, from) };
    let days = (b - a).num_days();
    let full = days / 7 * 5;
    let partial = (0..days % 7)
        .filter(|i| {
            let wd = (a.weekday().num_days_from_monday() as i64 + days / 7 * 7 + i) % 7;
            wd < 5
        })
        .count() as i64;
    full + partial
}

fn plural(n: i64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn span(from: NaiveDate, to: NaiveDate, unit: Count) -> String {
    let days = (to - from).num_days().abs();
    match unit {
        Count::Days => plural(days, "day", "days"),
        Count::Workdays => {
            let (a, b) = if from <= to { (from, to) } else { (to, from) };
            plural(
                workdays(a.succ_opt().unwrap_or(a), b),
                "workday",
                "workdays",
            )
        }
        Count::Weeks => match (days / 7, days % 7) {
            (w, 0) => plural(w, "week", "weeks"),
            (w, d) => format!(
                "{}, {}",
                plural(w, "week", "weeks"),
                plural(d, "day", "days")
            ),
        },
        Count::Months => {
            let (a, b) = if from <= to { (from, to) } else { (to, from) };
            let mut months = (b.year() - a.year()) * 12 + b.month() as i32 - a.month() as i32;
            if b.day() < a.day() {
                months -= 1;
            }
            let anchor = a
                .checked_add_months(Months::new(months.max(0) as u32))
                .unwrap_or(a);
            let rest = (b - anchor).num_days();
            match (months, rest) {
                (m, 0) => plural(m as i64, "month", "months"),
                (0, d) => plural(d, "day", "days"),
                (m, d) => format!(
                    "{}, {}",
                    plural(m as i64, "month", "months"),
                    plural(d, "day", "days")
                ),
            }
        }
    }
}

fn eval_span(line: &str, today: NaiveDate) -> Option<String> {
    let line = line.trim();
    if let Some(c) = phrase::month_count(line) {
        let month = c.month;
        let year = match c.year {
            Some(y) => y as i32,

            None if month + 2 < today.month() => today.year() + 1,
            None => today.year(),
        };
        let first = NaiveDate::from_ymd_opt(year, month, 1)?;
        let next = first.checked_add_months(Months::new(1))?;
        return Some(match count_unit(Some(&c.unit)) {
            Count::Workdays => plural(workdays(first, next), "workday", "workdays"),
            _ => plural((next - first).num_days(), "day", "days"),
        });
    }
    if let Some(c) = phrase::between(line) {
        let a = span_date(&c.a, today, true)?;
        let b = span_date(&c.b, today, true)?;
        let names_year = |t: &str| {
            t.split(|ch: char| !ch.is_ascii_digit())
                .any(|n| n.len() == 4)
        };

        let b = if b < a && !names_year(&c.b) {
            b.with_year(a.year() + 1).filter(|d| *d > a).unwrap_or(b)
        } else {
            b
        };
        let unit = count_unit(Some(&c.unit));
        return Some(if unit == Count::Workdays {
            plural(workdays(a.min(b), a.max(b)), "workday", "workdays")
        } else {
            span(a, b, unit)
        });
    }
    let c = phrase::until(line)?;
    let ahead = c.until;
    let date = span_date(&c.date, today, ahead)?;
    let unit = count_unit(c.unit.as_deref());
    if date == today {
        return Some("today".into());
    }
    let shown = span(today, date, unit);

    Some(match (ahead, date > today) {
        (true, false) => format!("{shown} ago"),
        (false, true) => format!("in {shown}"),
        _ => shown,
    })
}

fn clock(t: NaiveTime) -> String {
    let (pm, hour) = t.hour12();
    format!("{hour}:{:02} {}", t.minute(), if pm { "PM" } else { "AM" })
}

fn eval_date_math(line: &str, today: NaiveDate) -> Option<String> {
    if let Some(c) = phrase::date_shift(line) {
        let date = led_weekday(&c.from, today).or_else(|| this_years(&c.from, today))?;
        let n = if c.minus { -c.n } else { c.n };
        let moved = match c.unit {
            Unit::Day => date.checked_add_signed(Duration::try_days(n)?)?,
            Unit::Week => date.checked_add_signed(Duration::try_weeks(n)?)?,
            unit => {
                let months =
                    u32::try_from(n.unsigned_abs() * if unit == Unit::Year { 12 } else { 1 })
                        .ok()?;
                if n < 0 {
                    date.checked_sub_months(Months::new(months))?
                } else {
                    date.checked_add_months(Months::new(months))?
                }
            }
        };
        return Some(moved.format("%A, %-d %B %Y").to_string());
    }
    let c = phrase::date_diff(line)?;
    let date = |t: &str| led_weekday(t, today).or_else(|| this_years(t, today));
    let (a, b) = (date(&c.a)?, date(&c.b)?);
    let days = (a - b).num_days();
    let shown = plural(days.abs(), "day", "days");
    Some(if days < 0 { format!("-{shown}") } else { shown })
}

fn this_years(text: &str, today: NaiveDate) -> Option<NaiveDate> {
    let named = lone_date(text, NaiveDate::from_ymd_opt(today.year(), 1, 1)?, true)?;
    let from_today = lone_date(text, today, true)?;

    Some(
        if named.month() == from_today.month() && named.day() == from_today.day() {
            named
        } else {
            from_today
        },
    )
}

fn eval_date<Z: TimeZone>(line: &str, now: DateTime<Z>) -> Option<String> {
    let c = phrase::basic(line)?;
    let base = c.base;
    let wall = now.naive_local();
    let mut t = match base {
        Base::Tomorrow => wall.checked_add_days(Days::new(1))?,
        Base::Yesterday => wall.checked_sub_days(Days::new(1))?,
        _ => wall,
    };
    let mut show_time = base == Base::Now;
    if let Some((minus, value, unit)) = c.offset {
        let n = if minus { -value } else { value };
        let calendar = |days: Option<u64>, months: Option<u32>| match (days, months) {
            (Some(d), _) if n < 0 => t.checked_sub_days(Days::new(d)),
            (Some(d), _) => t.checked_add_days(Days::new(d)),
            (_, Some(m)) if n < 0 => t.checked_sub_months(Months::new(m)),
            (_, Some(m)) => t.checked_add_months(Months::new(m)),
            _ => None,
        };
        let size = n.unsigned_abs();
        t = match unit {
            Unit::Minute | Unit::Hour => {
                let span = if unit == Unit::Hour {
                    Duration::try_hours(n)?
                } else {
                    Duration::try_minutes(n)?
                };

                let then = now.checked_add_signed(span)?;
                then.naive_local()
            }
            Unit::Day => calendar(Some(size), None)?,
            Unit::Week => calendar(Some(size.checked_mul(7)?), None)?,
            Unit::Month => calendar(None, Some(u32::try_from(size).ok()?))?,
            Unit::Year => calendar(None, Some(u32::try_from(size.checked_mul(12)?).ok()?))?,
        };
        show_time |= matches!(unit, Unit::Minute | Unit::Hour);
    }
    Some(if show_time {
        t.format("%a, %-d %b %Y, %-I:%M %p").to_string()
    } else {
        t.format("%A, %-d %B %Y").to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 16, 12, 0, 0).unwrap()
    }

    fn conv(line: &str) -> Option<String> {
        eval_zone(line, now())
    }

    #[test]
    fn converts_between_zones() {
        assert!(conv("3pm PST in IST").unwrap().starts_with("3:30 AM IST"));
        assert!(conv("3pm PST in IST").unwrap().ends_with("(next day)"));
        assert_eq!(conv("10:30 London to New York").unwrap(), "5:30 AM EDT");
        assert_eq!(
            conv("Sunset 6:12pm IST in Sydney").unwrap(),
            "11:42 PM AEDT"
        );
        assert_eq!(conv("Standup 9:30am PST in IST").unwrap(), "10:00 PM IST");
        assert_eq!(conv("noon UTC in tokyo").unwrap(), "9:00 PM JST");
        assert_eq!(conv("9am IST in EST").unwrap(), "11:30 PM EDT (prev day)");
        assert_eq!(conv("7am Singapore in Dubai").unwrap(), "3:00 AM GST");
        assert_eq!(conv("noon UTC in Asia/Kabul").unwrap(), "4:30 PM UTC+0430");
    }

    #[test]
    fn counts_days() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 16).unwrap();
        let span = |l: &str| eval_span(l, today);
        assert_eq!(span("days until Dec 25").as_deref(), Some("70 days"));
        assert_eq!(span("until Christmas").as_deref(), Some("70 days"));
        assert_eq!(
            span("how many days until Christmas?").as_deref(),
            Some("70 days")
        );
        assert_eq!(span("weeks until Dec 25").as_deref(), Some("10 weeks"));
        assert_eq!(span("weeks until New Year's").as_deref(), Some("11 weeks"));
        assert_eq!(
            span("months until Dec 25").as_deref(),
            Some("2 months, 9 days")
        );
        assert_eq!(span("days since Jan 1").as_deref(), Some("288 days"));
        assert_eq!(
            span("days until Oct 1 2026").as_deref(),
            Some("15 days ago")
        );
        assert_eq!(span("days until today").as_deref(), Some("today"));
        assert_eq!(span("days until Friday").as_deref(), Some("today"));
        assert_eq!(span("days until next Friday").as_deref(), Some("7 days"));
        assert_eq!(span("days since Monday").as_deref(), Some("4 days"));

        assert_eq!(span("workdays until Oct 27").as_deref(), Some("6 workdays"));
        assert_eq!(
            span("days between Mar 1 2026 and Jun 30 2026").as_deref(),
            Some("121 days")
        );
        assert_eq!(
            span("workdays from Nov 2 2026 to Nov 9 2026").as_deref(),
            Some("5 workdays")
        );
        assert_eq!(
            span("days from Dec 20 to Jan 5").as_deref(),
            Some("16 days")
        );
        assert_eq!(span("days in February 2028").as_deref(), Some("29 days"));
        assert_eq!(span("workdays in November").as_deref(), Some("21 workdays"));
        assert_eq!(span("days until the meeting"), None);
    }

    #[test]
    fn converts_on_a_date() {
        assert_eq!(
            conv("3pm PST on Dec 1 in London").unwrap(),
            "11:00 PM GMT, Tue 1 Dec"
        );
        assert_eq!(
            conv("Dec 1 3pm PST in London").unwrap(),
            "11:00 PM GMT, Tue 1 Dec"
        );
        assert_eq!(
            conv("9am PST tomorrow in Tokyo").unwrap(),
            "1:00 AM JST, Sun 18 Oct"
        );
        assert_eq!(
            conv("on Friday 9am EST in London").unwrap(),
            "2:00 PM BST, Fri 16 Oct"
        );
        assert_eq!(
            conv("next Friday 9am EST in London").unwrap(),
            "2:00 PM BST, Fri 23 Oct"
        );
        assert_eq!(
            conv("Launch 2026-11-02 10am ET in Berlin").unwrap(),
            "4:00 PM CET, Mon 2 Nov"
        );
        assert_eq!(
            conv("Jan 5 9am PST in Tokyo").unwrap(),
            "2:00 AM JST, Wed 6 Jan 2027"
        );
        assert_eq!(
            conv("1 December 2026 noon UTC in NYC").unwrap(),
            "7:00 AM EST, Tue 1 Dec"
        );

        assert_eq!(conv("Wedding 4pm CET in New York").unwrap(), "10:00 AM EDT");
        assert_eq!(
            conv("Sunday brunch 11am PST in London").unwrap(),
            "7:00 PM BST, Sun 18 Oct"
        );
    }

    #[test]
    fn relative_times() {
        let rel = |line: &str| eval_relative(line, now());

        assert_eq!(rel("in 90 minutes in Tokyo").unwrap(), "10:30 PM JST");
        assert_eq!(rel("3 hours from now in UTC").unwrap(), "3:00 PM UTC");
        assert_eq!(rel("in 3 days in UTC").unwrap(), "12:00 PM UTC, Mon 19 Oct");
        assert_eq!(rel("in 1 hour 30 min in London").unwrap(), "2:30 PM BST");
        assert_eq!(rel("3pm UTC + 45 min").unwrap(), "3:45 PM UTC");
        assert_eq!(rel("11pm UTC + 2 hours").unwrap(), "1:00 AM UTC (next day)");
        assert_eq!(
            rel("9:30am PST + 2 hours in London").unwrap(),
            "7:30 PM BST"
        );
        assert_eq!(rel("Meeting 3pm UTC - 15 min").unwrap(), "2:45 PM UTC");

        assert!(rel("3pm + 45 min").unwrap().starts_with("3:45 PM"));
        assert!(rel("in 3 hours").unwrap().ends_with('M'));

        assert_eq!(rel("3 + 45 min"), None);
        assert_eq!(rel("$5 + 3 hours"), None);
        assert_eq!(rel("in 3 m"), None);
    }

    #[test]
    fn huge_spans_are_not_times() {
        let rel = |line: &str| eval_relative(line, now());
        assert_eq!(rel("in 99999999999 h"), None);
        assert_eq!(rel("3pm UTC + 99999999999 hours"), None);
        assert_eq!(eval_date("today + 99999999999999 days", Local::now()), None);
        assert_eq!(eval_date("now + 999999999999 hours", Local::now()), None);
    }

    #[test]
    fn long_shifts_say_which_day() {
        let rel = |line: &str| eval_relative(line, now());
        assert_eq!(rel("3pm UTC + 3 days").unwrap(), "3:00 PM UTC, Mon 19 Oct");
        assert_eq!(
            rel("9am PST + 2 days in London").unwrap(),
            "5:00 PM BST, Sun 18 Oct"
        );
        assert_eq!(
            rel("in 100 weeks in UTC").unwrap(),
            "12:00 PM UTC, Fri 15 Sep 2028"
        );
    }

    #[test]
    fn gmt_is_a_fixed_offset() {
        assert_eq!(conv("3pm GMT in UTC").unwrap(), "3:00 PM UTC");
    }

    #[test]
    fn current_time_in_zone() {
        assert_eq!(conv("time in Tokyo").unwrap(), "9:00 PM JST");
        assert_eq!(conv("now in India").unwrap(), "5:30 PM IST");
    }

    #[test]
    fn leaves_math_alone() {
        assert_eq!(conv("3 km in miles"), None);
        assert_eq!(conv("255 in hex"), None);
        assert_eq!(conv("15 in IST"), None);
        assert_eq!(conv("$30 in EUR"), None);
    }

    #[test]
    fn days_cross_clock_changes_on_the_calendar() {
        let ny: Tz = "America/New_York".parse().unwrap();
        let at = |y, mo, d, h, mi| ny.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap();
        let date = |line: &str, now| eval_date(line, now).unwrap();
        let before_fall = at(2026, 10, 18, 0, 30);
        assert_eq!(
            date("today + 3 weeks", before_fall),
            "Sunday, 8 November 2026"
        );
        assert_eq!(
            date("today + 21 days", before_fall),
            "Sunday, 8 November 2026"
        );
        assert_eq!(
            date("tomorrow + 2 weeks", before_fall),
            "Monday, 2 November 2026"
        );
        assert_eq!(
            date("today + 1 month", before_fall),
            "Wednesday, 18 November 2026"
        );
        let late = at(2026, 10, 31, 23, 30);
        assert_eq!(date("tomorrow", late), "Sunday, 1 November 2026");
        assert_eq!(date("today + 1 day", late), "Sunday, 1 November 2026");

        let afternoon = at(2026, 10, 30, 15, 0);
        assert_eq!(date("now + 3 days", afternoon), "Mon, 2 Nov 2026, 3:00 PM");
        assert_eq!(
            date("now + 72 hours", afternoon),
            "Mon, 2 Nov 2026, 2:00 PM"
        );
        assert_eq!(
            date("now - 90 minutes", afternoon),
            "Fri, 30 Oct 2026, 1:30 PM"
        );

        let spring = at(2026, 3, 9, 0, 30);
        assert_eq!(date("today - 1 week", spring), "Monday, 2 March 2026");
        assert_eq!(date("yesterday", spring), "Sunday, 8 March 2026");
        assert_eq!(date("now - 24 hours", spring), "Sat, 7 Mar 2026, 11:30 PM");

        for zone in [
            "Europe/London",
            "Australia/Sydney",
            "Asia/Kolkata",
            "America/Los_Angeles",
        ] {
            let tz: Tz = zone.parse().unwrap();
            for (m, d) in [(3, 20), (3, 30), (4, 1), (10, 20), (10, 27), (11, 1)] {
                let now = tz.with_ymd_and_hms(2026, m, d, 0, 15, 0).unwrap();
                for n in [1u64, 6, 7, 14, 21] {
                    let want = now.date_naive().checked_add_days(Days::new(n)).unwrap();
                    assert_eq!(
                        date(&format!("today + {n} days"), now),
                        want.format("%A, %-d %B %Y").to_string(),
                        "{zone} {m}/{d} + {n}"
                    );
                }
            }
        }
    }

    #[test]
    fn relative_days_cross_clock_changes_on_the_calendar() {
        let now = Utc.with_ymd_and_hms(2026, 10, 30, 19, 0, 0).unwrap();
        let rel = |line: &str| eval_relative(line, now).unwrap();
        assert_eq!(rel("in 3 days in New York"), "3:00 PM EST, Mon 2 Nov");
        assert_eq!(rel("in 72 hours in New York"), "2:00 PM EST, Mon 2 Nov");
        assert_eq!(rel("in 1 week in New York"), "3:00 PM EST, Fri 6 Nov");
        assert_eq!(
            rel("in 2 days 3 hours in New York"),
            "6:00 PM EST, Sun 1 Nov"
        );
    }

    #[test]
    fn weekday_with_a_word_in_date_maths() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 18).unwrap();
        let math = |l: &str| eval_date_math(l, today);
        assert_eq!(
            math("last friday + 1 week").as_deref(),
            Some("Friday, 23 October 2026")
        );
        assert_eq!(
            math("last Friday - 2 days").as_deref(),
            Some("Wednesday, 14 October 2026")
        );
        assert_eq!(
            math("this Friday + 1 day").as_deref(),
            Some("Saturday, 24 October 2026")
        );
        assert_eq!(
            math("on Friday + 1 month").as_deref(),
            Some("Monday, 23 November 2026")
        );
        assert_eq!(
            math("Dec 25 2026 - last Friday").as_deref(),
            Some("70 days")
        );

        let friday = NaiveDate::from_ymd_opt(2026, 10, 23).unwrap();
        assert_eq!(
            eval_date_math("last friday + 1 day", friday).as_deref(),
            Some("Saturday, 17 October 2026")
        );
        assert_eq!(
            eval_date_math("this friday + 1 day", friday).as_deref(),
            Some("Saturday, 24 October 2026")
        );
        assert_eq!(math("last week + 1 day"), None);
    }

    #[test]
    fn dates_on_their_own() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 18).unwrap();
        let lone = |l: &str| eval_lone(l, today);
        assert_eq!(lone("Dec 25").as_deref(), Some("Friday, 25 December 2026"));
        assert_eq!(lone("Jan 5").as_deref(), Some("Monday, 5 January 2026"));
        assert_eq!(
            lone("next friday").as_deref(),
            Some("Friday, 23 October 2026")
        );
        assert_eq!(
            lone("this Friday").as_deref(),
            Some("Friday, 23 October 2026")
        );
        assert_eq!(
            lone("last friday").as_deref(),
            Some("Friday, 16 October 2026")
        );
        assert_eq!(
            lone("last sunday").as_deref(),
            Some("Sunday, 11 October 2026")
        );
        assert_eq!(
            lone("next sunday").as_deref(),
            Some("Sunday, 25 October 2026")
        );
        assert_eq!(
            lone("on monday").as_deref(),
            Some("Monday, 19 October 2026")
        );
        assert_eq!(lone("Friday"), None);
        assert_eq!(lone("this Dec 25"), None);
        assert_eq!(lone("Dec 25 $40"), None);
        let span = |l: &str| eval_span(l, today);
        assert_eq!(span("days until 12/25").as_deref(), Some("68 days"));
        assert_eq!(span("days until 25/12").as_deref(), Some("68 days"));
        assert_eq!(span("days until 3/3").as_deref(), Some("136 days"));
        assert_eq!(span("days since 1/15/2026").as_deref(), Some("276 days"));
        assert_eq!(span("days since 15/1/26").as_deref(), Some("276 days"));
        assert_eq!(span("days until 2/3"), None);
        assert_eq!(
            note("days until 2/3"),
            Some("Write the month: Feb 3 or 3 Feb")
        );
        assert_eq!(note("days until 12/25"), None);
        assert_eq!(note("2/3"), None);
    }
}
