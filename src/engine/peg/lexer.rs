use std::collections::HashSet;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Mod,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Func {
    Sqrt,
    Cbrt,
    Abs,
    Round,
    Ceil,
    Floor,
    Ln,
    Log,
    Fact,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Sinh,
    Cosh,
    Tanh,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Of,
    On,
    Off,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Repr {
    Hex,
    Binary,
    Octal,
    Decimal,
    Scientific,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Eof,
    Num { text: String, value: f64 },
    Base { value: f64, repr: Repr },
    Pct,
    Op(BinOp),
    Assign,
    PlusEq,
    MinusEq,
    MulEq,
    DivEq,
    Bang,
    LParen,
    RParen,
    Comma,
    Word(String),
    Unit(&'static str),
    Func(Func),
    Repr(Repr),
    Scale(f64),
    Cur(&'static str),
    CurWord(&'static str, String),
    Convert,
    In,
    From,
    Dir(Dir),
    Rev(Dir),
    AsPct(Dir),
    Aggregate,
}

pub const PROSE: &[&str] = &[
    "at",
    "per",
    "a",
    "an",
    "the",
    "my",
    "our",
    "your",
    "his",
    "her",
    "it",
    "its",
    "i",
    "we",
    "you",
    "they",
    "he",
    "she",
    "split",
    "ways",
    "between",
    "among",
    "across",
    "tip",
    "tip",
    "pre",
    "post",
    "money",
    "version",
    "imperial",
    "uk",
    "british",
    "american",
    "us",
    "royal",
    "metric",
    "ago",
    "since",
    "until",
    "after",
    "before",
    "during",
    "every",
    "except",
    "next",
    "last",
    "this",
    "double",
    "twice",
    "triple",
    "half",
    "third",
    "quarter",
    "quarter",
    "approx",
    "about",
    "roughly",
    "nearly",
    "left",
    "remaining",
    "change",
    "each",
    "person",
    "people",
    "friends",
    "guests",
];

pub const CURRENCY_CODES: &[&str] = &[
    "USD", "EUR", "GBP", "JPY", "INR", "CNY", "AUD", "CAD", "CHF", "HKD", "SGD", "SEK", "NOK",
    "DKK", "KRW", "RUB", "TRY", "BRL", "MXN", "ZAR", "ILS", "PLN", "CZK", "HUF", "THB", "IDR",
    "MYR", "PHP", "AED", "SAR", "QAR", "KWD", "BHD", "OMR", "EGP", "NGN", "KES", "GHS", "TND",
    "MAD", "VND", "TWD", "NZD", "UAH", "RSD", "RON", "BGN", "HRK", "ISK", "PKR", "BDT", "LKR",
    "NPR", "KZT", "UZS", "AZN", "GEL", "AMD", "JOD", "LBP", "IQD", "IRR", "COP", "ARS", "CLP",
    "PEN", "UYU", "PYG", "BOB", "CRC", "GTQ", "DOP", "XAF", "XOF", "XPF", "FJD", "TOP", "WST",
    "LAK", "KHR", "MNT", "BND", "BWP", "NAD", "SZL", "ZMW", "UGX", "TZS", "ETB", "MUR", "SCR",
    "ISK", "ALL", "MKD", "BAM", "MDL", "TJS", "TMT", "KGS", "PGK", "SBD", "LRD", "SLL", "GMD",
    "RWF", "BIF", "CDF", "MGA", "MWK", "MZM", "AOA", "CVE", "STN", "GNF", "KMF", "DJF", "SOS",
    "LYD", "SYP", "YER", "AFN", "TMM", "BTN", "MVR", "HTG", "HNL", "NIO", "NOK", "SYP",
];

pub const MONEY_SCALES: &[(&str, f64)] = &[
    ("mm", 1e6),
    ("bn", 1e9),
    ("bil", 1e9),
    ("billion", 1e9),
    ("millions", 1e6),
    ("million", 1e6),
    ("thousands", 1e3),
    ("thousand", 1e3),
    ("m", 1e6),
    ("b", 1e9),
    ("k", 1e3),
];

pub struct Token {
    pub kind: Kind,
    pub glued: bool,
}

const LITERALS: &[(&str, Kind)] = &[
    ("as a percent increase of", Kind::AsPct(Dir::On)),
    ("as a percentage increase of", Kind::AsPct(Dir::On)),
    ("as a percent decrease of", Kind::AsPct(Dir::Off)),
    ("as a percentage decrease of", Kind::AsPct(Dir::Off)),
    ("as a percentage of", Kind::AsPct(Dir::Of)),
    ("as a percent of", Kind::AsPct(Dir::Of)),
    ("as a percentage", Kind::AsPct(Dir::Of)),
    ("as a % increase of", Kind::AsPct(Dir::On)),
    ("as a % decrease of", Kind::AsPct(Dir::Off)),
    ("as a % of", Kind::AsPct(Dir::Of)),
    ("as a percent on", Kind::AsPct(Dir::On)),
    ("as a % on", Kind::AsPct(Dir::On)),
    ("as a percent off", Kind::AsPct(Dir::Off)),
    ("as a % off", Kind::AsPct(Dir::Off)),
    ("multiplied by", Kind::Op(BinOp::Mul)),
    ("divided by", Kind::Op(BinOp::Div)),
    ("divide by", Kind::Op(BinOp::Div)),
    ("square roots", Kind::Func(Func::Sqrt)),
    ("square root", Kind::Func(Func::Sqrt)),
    ("cubic root", Kind::Func(Func::Cbrt)),
    ("cubic root", Kind::Func(Func::Cbrt)),
    ("cubed root", Kind::Func(Func::Cbrt)),
    ("cube root", Kind::Func(Func::Cbrt)),
    ("of what is", Kind::Rev(Dir::Of)),
    ("on what is", Kind::Rev(Dir::On)),
    ("off what is", Kind::Rev(Dir::Off)),
    ("per cent", Kind::Pct),
    ("nautical miles", Kind::Unit("nautical_mile")),
    ("nautical mile", Kind::Unit("nautical_mile")),
    ("fluid ounces", Kind::Unit("floz")),
    ("fluid ounce", Kind::Unit("floz")),
    ("short tons", Kind::Unit("short_ton")),
    ("short ton", Kind::Unit("short_ton")),
    ("metric tons", Kind::Unit("tonne")),
    ("metric ton", Kind::Unit("tonne")),
    ("metric tonnes", Kind::Unit("tonne")),
    ("metric tonne", Kind::Unit("tonne")),
    ("square feet", Kind::Unit("ft^2")),
    ("square foot", Kind::Unit("ft^2")),
    ("square meters", Kind::Unit("m^2")),
    ("square metres", Kind::Unit("m^2")),
    ("square meter", Kind::Unit("m^2")),
    ("square metre", Kind::Unit("m^2")),
    ("us gallons", Kind::Unit("gallon")),
    ("us gallon", Kind::Unit("gallon")),
    ("is equal to", Kind::Assign),
    ("equal to", Kind::Assign),
    ("equals to", Kind::Assign),
    ("sq ft", Kind::Unit("ft^2")),
    ("sq m", Kind::Unit("m^2")),
    ("fl oz", Kind::Unit("floz")),
];

const WORD_OPS: &[(&str, BinOp)] = &[
    ("divided", BinOp::Div),
    ("divide", BinOp::Div),
    ("without", BinOp::Sub),
    ("subtract", BinOp::Sub),
    ("modulo", BinOp::Mod),
    ("times", BinOp::Mul),
    ("multiplied", BinOp::Mul),
    ("multiply", BinOp::Mul),
    ("minus", BinOp::Sub),
    ("plus", BinOp::Add),
    ("mult", BinOp::Mul),
    ("mul", BinOp::Mul),
    ("with", BinOp::Add),
    ("mod", BinOp::Mod),
    ("and", BinOp::Add),
    ("xor", BinOp::BitXor),
];

const FUNCS: &[(&str, Func)] = &[
    ("sqrt", Func::Sqrt),
    ("cbrt", Func::Cbrt),
    ("abs", Func::Abs),
    ("round", Func::Round),
    ("ceil", Func::Ceil),
    ("ceiling", Func::Ceil),
    ("floor", Func::Floor),
    ("factorial", Func::Fact),
    ("arcsin", Func::Asin),
    ("arccos", Func::Acos),
    ("arctan", Func::Atan),
    ("asin", Func::Asin),
    ("acos", Func::Acos),
    ("atan", Func::Atan),
    ("sinh", Func::Sinh),
    ("cosh", Func::Cosh),
    ("tanh", Func::Tanh),
    ("fact", Func::Fact),
    ("log", Func::Log),
    ("sin", Func::Sin),
    ("cos", Func::Cos),
    ("tan", Func::Tan),
    ("ln", Func::Ln),
];

const SCALES: &[(&str, f64)] = &[
    ("septillions", 1e24),
    ("septillion", 1e24),
    ("sextillions", 1e21),
    ("sextillion", 1e21),
    ("quintillions", 1e18),
    ("quintillion", 1e18),
    ("quadrillions", 1e15),
    ("quadrillion", 1e15),
    ("trillions", 1e12),
    ("trillion", 1e12),
    ("milliards", 1e9),
    ("milliard", 1e9),
    ("billions", 1e9),
    ("billion", 1e9),
    ("millions", 1e6),
    ("million", 1e6),
    ("thousands", 1e3),
    ("thousand", 1e3),
    ("crores", 1e7),
    ("crore", 1e7),
    ("lakhs", 1e5),
    ("lakh", 1e5),
    ("lacs", 1e5),
    ("lac", 1e5),
    ("M", 1e6),
    ("k", 1e3),
];

const REPRS: &[(&str, Repr)] = &[
    ("hexadecimal", Repr::Hex),
    ("scientific", Repr::Scientific),
    ("exponential", Repr::Scientific),
    ("octal", Repr::Octal),
    ("decimal", Repr::Decimal),
    ("hex", Repr::Hex),
    ("binary", Repr::Binary),
    ("bin", Repr::Binary),
    ("oct", Repr::Octal),
    ("dec", Repr::Decimal),
    ("sci", Repr::Scientific),
    ("exp", Repr::Scientific),
];

const PERCENT_WORDS: &[&str] = &["percents", "percent", "pcts", "pct"];

const UNIT_FIXES: &[(&str, &str)] = &[
    ("mb", "megabit"),
    ("mbs", "megabits"),
    ("mbit", "megabit"),
    ("mbits", "megabits"),
];

const AGGREGATES: &[&str] = &[
    "average", "previous", "total", "sum", "avg", "mean", "min", "max", "prev",
];

const CONVERSIONS: &[&str] = &["into", "to", "as"];

const ASSIGN_WORDS: &[&str] = &["is", "was", "equal", "equals", "amounts to", "makes"];

const CURRENCY_SYMBOLS: &[(&str, &str)] = &[
    ("NT$", "TWD"),
    ("HK$", "HKD"),
    ("US$", "USD"),
    ("CA$", "CAD"),
    ("AU$", "AUD"),
    ("NZ$", "NZD"),
    ("A$", "AUD"),
    ("C$", "CAD"),
    ("S$", "SGD"),
    ("R$", "BRL"),
    ("$", "USD"),
    ("€", "EUR"),
    ("£", "GBP"),
    ("¥", "JPY"),
    ("₹", "INR"),
    ("₩", "KRW"),
    ("₽", "RUB"),
    ("₺", "TRY"),
    ("₴", "UAH"),
    ("₫", "VND"),
    ("₦", "NGN"),
    ("₪", "ILS"),
    ("₱", "PHP"),
    ("₨", "PKR"),
    ("﷼", "SAR"),
    ("៛", "KHR"),
    ("₡", "CRC"),
    ("₸", "KZT"),
    ("₼", "AZN"),
    ("৳", "BDT"),
    ("₭", "LAK"),
];

const CURRENCY_WORDS: &[(&str, &str)] = &[
    ("dirhams", "AED"),
    ("dirham", "AED"),
    ("riyals", "SAR"),
    ("riyal", "SAR"),
    ("sfr.", "CHF"),
    ("sfr", "CHF"),
    ("nis", "ILS"),
    ("dh", "AED"),
    ("sr", "SAR"),
];

pub fn tokenize(line: &str, vars: &HashSet<String>) -> Option<Vec<Token>> {
    let mut lexer = Lexer {
        text: line.chars().collect(),
        at: 0,
        vars,
        value_end: false,
        last_end: 0,
    };
    let mut out = Vec::new();
    loop {
        let glued = lexer.last_end != 0 && lexer.last_end == lexer.at;
        let Some(kind) = lexer.next() else {
            return None;
        };
        let done = kind == Kind::Eof;
        let ends_value = matches!(
            kind,
            Kind::Num { .. }
                | Kind::Base { .. }
                | Kind::Word(_)
                | Kind::Unit(_)
                | Kind::Cur(_)
                | Kind::CurWord(_, _)
                | Kind::Pct
                | Kind::Scale(_)
                | Kind::RParen
                | Kind::Bang
        );
        out.push(Token { kind, glued });
        lexer.value_end = ends_value;
        lexer.last_end = lexer.at;
        if done {
            break;
        }
        if out.len() > 400 {
            return None;
        }
    }
    Some(out)
}

struct Lexer<'a> {
    text: Vec<char>,
    at: usize,
    vars: &'a HashSet<String>,
    value_end: bool,
    last_end: usize,
}

impl<'a> Lexer<'a> {
    fn peek(&self, ahead: usize) -> Option<char> {
        self.text.get(self.at + ahead).copied()
    }

    fn skip_spaces(&mut self) {
        while matches!(self.peek(0), Some(' ') | Some('\t')) {
            self.at += 1;
        }
    }

    fn rest_lower(&self) -> String {
        self.text[self.at..]
            .iter()
            .map(|c| c.to_ascii_lowercase())
            .collect()
    }

    fn literal(&mut self) -> Option<Kind> {
        let rest = self.rest_lower();
        for (words, kind) in LITERALS {
            if !rest.starts_with(words) {
                continue;
            }
            let end = words.chars().count();
            if rest[end..].starts_with(|c: char| c.is_alphanumeric()) {
                continue;
            }
            self.at += end;
            return Some(kind.clone());
        }
        None
    }

    fn currency_symbol(&mut self) -> Option<(&'static str, &'static str)> {
        let head: String = self.text[self.at..self.at + 4.min(self.text.len() - self.at)]
            .iter()
            .collect();
        let previous = if self.at == 0 {
            None
        } else {
            self.text.get(self.at - 1).copied()
        };
        for (sym, code) in CURRENCY_SYMBOLS {
            if !head.starts_with(sym) {
                continue;
            }
            let compound = sym.chars().count() > 1;
            let after = self.text.get(self.at + sym.chars().count()).copied();
            let amount_follows = after.is_some_and(|c| {
                c.is_ascii_digit() || c == '.' || c == '(' || c == '-' || c == ' ' || c == '\t'
            });
            if compound {
                if !amount_follows {
                    continue;
                }
                if previous.is_some_and(|c| c.is_alphanumeric()) {
                    continue;
                }
            }
            return Some((sym, code));
        }
        None
    }

    fn next(&mut self) -> Option<Kind> {
        self.skip_spaces();
        let Some(c) = self.peek(0) else {
            return Some(Kind::Eof);
        };

        if c == '/' && self.peek(1) == Some('/') {
            return Some(Kind::Eof);
        }
        if c == '#' {
            return Some(Kind::Eof);
        }
        if c == '"' {
            let mut at = self.at + 1;
            while at < self.text.len() && self.text[at] != '"' {
                at += 1;
            }
            if at >= self.text.len() {
                return Some(Kind::Eof);
            }
            self.at = at + 1;
            return self.next();
        }

        if let Some(kind) = self.currency_symbol() {
            let (sym, code) = kind;
            self.at += sym.chars().count();
            return Some(Kind::Cur(code));
        }
        if let Some(kind) = self.literal() {
            return Some(kind);
        }

        match c {
            '\n' | '\r' => Some(Kind::Eof),
            '=' => self.bump(Kind::Assign),
            '+' if self.peek(1) == Some('=') => self.bump2(Kind::PlusEq),
            '-' if self.peek(1) == Some('=') => self.bump2(Kind::MinusEq),
            '*' if self.peek(1) == Some('=') => self.bump2(Kind::MulEq),
            '/' if self.peek(1) == Some('=') => self.bump2(Kind::DivEq),
            '+' => self.bump(Kind::Op(BinOp::Add)),
            '-' => self.bump(Kind::Op(BinOp::Sub)),
            '*' => self.bump(Kind::Op(BinOp::Mul)),
            '/' => self.bump(Kind::Op(BinOp::Div)),
            '^' => self.bump(Kind::Op(BinOp::Pow)),
            '&' => self.bump(Kind::Op(BinOp::BitAnd)),
            '|' => self.bump(Kind::Op(BinOp::BitOr)),
            '<' if self.peek(1) == Some('<') => self.bump2(Kind::Op(BinOp::Shl)),
            '>' if self.peek(1) == Some('>') => self.bump2(Kind::Op(BinOp::Shr)),
            '×' => self.bump(Kind::Op(BinOp::Mul)),
            '÷' => self.bump(Kind::Op(BinOp::Div)),
            '%' => self.bump(Kind::Pct),
            '(' => self.bump(Kind::LParen),
            ')' => self.bump(Kind::RParen),
            '!' => self.bump(Kind::Bang),
            ',' => self.bump(Kind::Comma),
            ':' => self.bump(Kind::Comma),
            c if c.is_ascii_digit()
                || (c == '.' && self.peek(1).is_some_and(|d| d.is_ascii_digit())) =>
            {
                self.number()
            }
            c if c.is_alphabetic() || c == '_' => self.word(),
            _ => None,
        }
    }

    fn bump(&mut self, kind: Kind) -> Option<Kind> {
        self.at += 1;
        Some(kind)
    }

    fn bump2(&mut self, kind: Kind) -> Option<Kind> {
        self.at += 2;
        Some(kind)
    }

    fn number(&mut self) -> Option<Kind> {
        if self.peek(0) == Some('0') {
            match self.peek(1) {
                Some('x') | Some('X') => {
                    if let Some(value) = self.radix(16, 2, |d| d.is_ascii_hexdigit()) {
                        return Some(Kind::Base {
                            value,
                            repr: Repr::Hex,
                        });
                    }
                }
                Some('b') => {
                    if let Some(value) = self.radix(2, 2, |d| d == '0' || d == '1') {
                        return Some(Kind::Base {
                            value,
                            repr: Repr::Binary,
                        });
                    }
                }
                Some('o') | Some('O') => {
                    if let Some(value) = self.radix(8, 2, |d| ('0'..='7').contains(&d)) {
                        return Some(Kind::Base {
                            value,
                            repr: Repr::Octal,
                        });
                    }
                }
                _ => {}
            }
        }

        let start = self.at;
        let mut digits = String::new();
        let mut separated = false;
        while let Some(c) = self.peek(0) {
            if c.is_ascii_digit() {
                digits.push(c);
                self.at += 1;
                continue;
            }
            if c == ',' && self.three_digits(1) {
                separated = true;
                self.at += 1;
                continue;
            }
            if c == ' ' && self.space_run() > 0 && self.three_digits(self.space_run()) {
                let after = self.peek(self.space_run() + 3);
                if after.is_none_or(|c| !c.is_alphanumeric() && c != '.' && c != ',') {
                    separated = true;
                    self.at += self.space_run();
                    continue;
                }
            }

            if c == '.' && self.three_digits(1) && self.peek(4) == Some('.') {
                separated = true;
                self.at += 1;
                continue;
            }
            break;
        }
        if digits.is_empty() && self.peek(0) != Some('.') {
            self.at = start;
            return None;
        }

        let mut fraction = String::new();
        if self.peek(0) == Some('.') && self.peek(1).is_some_and(|d| d.is_ascii_digit()) {
            self.at += 1;
            while let Some(d) = self.peek(0) {
                if !d.is_ascii_digit() {
                    break;
                }
                fraction.push(d);
                self.at += 1;
            }
        }

        let mut exponent: Option<i64> = None;
        if matches!(self.peek(0), Some('e') | Some('E')) {
            let mut at = self.at + 1;
            let mut sign = 1i64;
            if matches!(self.text.get(at), Some('+')) {
                at += 1;
            } else if matches!(self.text.get(at), Some('-')) {
                sign = -1;
                at += 1;
            }
            let mut run = String::new();
            while matches!(self.text.get(at), Some(d) if d.is_ascii_digit()) {
                run.push(self.text[at]);
                at += 1;
            }
            if let Ok(power) = run.parse::<i64>() {
                exponent = Some(sign * power);
                self.at = at;
            }
        }

        let grouped = separated || self.text[start..self.at].contains(&' ');
        let body = if grouped {
            let fraction = if fraction.is_empty() {
                String::new()
            } else {
                format!(".{fraction}")
            };
            let exponent = match exponent {
                Some(power) => format!("e{power}"),
                None => String::new(),
            };
            format!("{digits}{fraction}{exponent}")
        } else {
            self.text[start..self.at].iter().collect::<String>()
        };
        let value = body.parse::<f64>().ok()?;
        Some(Kind::Num { text: body, value })
    }

    fn space_run(&self) -> usize {
        let mut run = 0;
        while matches!(self.text.get(self.at + run), Some(' ') | Some('\t')) {
            run += 1;
        }
        run
    }

    fn three_digits(&self, ahead: usize) -> bool {
        (0..3).all(|i| matches!(self.peek(ahead + i), Some(d) if d.is_ascii_digit()))
    }

    fn radix(&mut self, base: u32, skip: usize, valid: impl Fn(char) -> bool) -> Option<f64> {
        let mut at = self.at + skip;
        let mut digits = String::new();
        while matches!(self.text.get(at), Some(d) if valid(*d)) {
            digits.push(self.text[at]);
            at += 1;
        }
        if digits.is_empty() {
            return None;
        }
        let mut value = 0f64;
        for d in digits.chars() {
            value = value * base as f64 + d.to_digit(base)? as f64;
        }
        self.at = at;
        Some(value)
    }

    fn word(&mut self) -> Option<Kind> {
        let start = self.at;
        while matches!(self.peek(0), Some(c) if c.is_alphanumeric() || c == '_') {
            self.at += 1;
        }

        while self.peek(0) == Some('.')
            && self
                .peek(1)
                .is_none_or(|c| !(c.is_ascii_digit() || c.is_alphabetic()))
        {
            self.at += 1;
        }
        let word: String = self.text[start..self.at].iter().collect();
        let lower = word.to_lowercase();

        if self.value_end
            && matches!(word.as_str(), "x" | "X")
            && !self.vars.contains(&word)
            && self
                .peek(0)
                .is_some_and(|c| c.is_ascii_digit() || c == '(' || c == '.' || c == '-' || c == ' ')
        {
            return Some(Kind::Op(BinOp::Mul));
        }

        for (name, op) in WORD_OPS {
            if lower == *name {
                return Some(Kind::Op(*op));
            }
        }

        let unspaced = lower.trim_end_matches('.');
        for name in PERCENT_WORDS {
            if lower == *name || unspaced == *name {
                return Some(Kind::Pct);
            }
        }
        for name in AGGREGATES {
            if lower == *name {
                return Some(Kind::Aggregate);
            }
        }
        for (name, func) in FUNCS {
            if lower == *name {
                return Some(Kind::Func(*func));
            }
        }
        for (name, repr) in REPRS {
            if lower == *name {
                return Some(Kind::Repr(*repr));
            }
        }
        for (name, factor) in SCALES {
            let exact = if name.len() <= 2 {
                word == **name
            } else {
                lower == **name
            };
            if exact {
                return Some(Kind::Scale(*factor));
            }
        }
        for (name, code) in CURRENCY_WORDS {
            if lower == *name {
                return Some(Kind::CurWord(code, word));
            }
        }
        for name in ASSIGN_WORDS {
            if lower == *name {
                return Some(Kind::Assign);
            }
        }

        if let Some(unit) = UNIT_FIXES
            .iter()
            .find_map(|(from, to)| (*from == word.as_str()).then_some(*to))
        {
            return Some(Kind::Unit(unit));
        }
        match lower.as_str() {
            "in" => Some(Kind::In),
            "from" => Some(Kind::From),
            "of" => Some(Kind::Dir(Dir::Of)),
            "on" => Some(Kind::Dir(Dir::On)),
            "off" => Some(Kind::Dir(Dir::Off)),
            _ if CONVERSIONS.contains(&lower.as_str()) => Some(Kind::Convert),
            _ => Some(Kind::Word(word)),
        }
    }
}
