use super::format_number;

pub const SYMBOLS: &[(&str, &str)] = &[
    ("USD", "$"),
    ("GBP", "£"),
    ("EUR", "€"),
    ("JPY", "¥"),
    ("INR", "₹"),
    ("KRW", "₩"),
    ("RUB", "₽"),
    ("TRY", "₺"),
    ("UAH", "₴"),
    ("VND", "₫"),
    ("NGN", "₦"),
    ("ILS", "₪"),
    ("PHP", "₱"),
    ("PKR", "₨"),
    ("BRL", "R$"),
    ("CAD", "C$"),
    ("AUD", "A$"),
    ("NZD", "NZ$"),
    ("HKD", "HK$"),
    ("SGD", "S$"),
    ("TWD", "NT$"),
    ("MXN", "MXN$"),
    ("CNY", "CN¥"),
    ("THB", "฿"),
    ("KHR", "៛"),
    ("BDT", "৳"),
    ("LAK", "₭"),
    ("CRC", "₡"),
    ("KZT", "₸"),
    ("AZN", "₼"),
    ("PLN", "zł"),
    ("KES", "KSh"),
    ("GHS", "GH₵"),
    ("IDR", "Rp"),
    ("MYR", "RM"),
    ("SEK", "kr"),
    ("NOK", "kr"),
    ("DKK", "kr"),
    ("RON", "lei"),
    ("HUF", "Ft"),
];

pub fn symbol(code: &str) -> Option<&'static str> {
    SYMBOLS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(code))
        .map(|(_, sym)| *sym)
}

pub fn currency(sym: &str) -> Option<&'static str> {
    SYMBOLS
        .iter()
        .rev()
        .find(|(_, name)| *name == sym)
        .map(|(code, _)| *code)
}

const MARKS: &[char] = &[
    '$', '£', '€', '¥', '₹', '₩', '₽', '₺', '₴', '₫', '₦', '₪', '₱',
];

fn is_code(token: &str) -> bool {
    token.len() == 3
        && token.chars().all(char::is_uppercase)
        && token.chars().all(char::is_alphabetic)
}

fn digits(token: &str) -> Option<(bool, String, Option<String>)> {
    let body = token
        .trim_start_matches(['$', '£', '€', '¥', '₹', '-'])
        .trim_end_matches(['$', '£', '€', '¥', '₹'])
        .replace(',', "");
    if body.is_empty()
        || !body.chars().all(|c| c.is_ascii_digit() || c == '.')
        || !body.chars().any(|c| c.is_ascii_digit())
    {
        return None;
    }
    let negative = token.starts_with('-') || token.contains("$-");
    let (int, frac) = match body.split_once('.') {
        Some((int, frac)) => (int.to_string(), Some(frac.to_string())),
        None => (body.clone(), None),
    };
    Some((negative, int, frac))
}

pub fn money_view(raw: &str) -> Option<String> {
    let body = raw.trim_start_matches("approx. ");
    let tokens: Vec<&str> = body.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }

    let code = tokens
        .iter()
        .position(|t| is_code(t))
        .map(|at| tokens[at].to_string())
        .or_else(|| {
            tokens.iter().find_map(|t| {
                let mark = t.chars().find(|c| MARKS.contains(c))?;
                currency(&mark.to_string()).map(str::to_string)
            })
        })?;

    let at = tokens
        .iter()
        .position(|t| digits(t).is_some() && !is_code(t))?;
    let (negative, int, frac) = digits(tokens[at])?;

    let amount = format_number(&int, frac.as_deref(), true);
    let sign = if negative { "-" } else { "" };
    let head = match symbol(&code) {
        Some(sym) => format!("{sign}{sym}{amount}"),
        None => format!("{sign}{amount} {code}"),
    };

    let tail: Vec<&str> = tokens
        .iter()
        .enumerate()
        .filter(|(idx, token)| *idx != at && !is_code(token) && digits(token).is_none())
        .map(|(_, token)| *token)
        .collect();

    if tail.is_empty() {
        Some(head)
    } else {
        Some(format!("{head} {}", tail.join(" ")))
    }
}
