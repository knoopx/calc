mod lexer;
mod lower;
mod parse;

pub mod time;

use std::collections::HashSet;

pub use parse::{AssignOp, Ast};

#[derive(Clone, Debug)]
pub struct Program {
    pub fend: String,
    pub name: Option<String>,
    pub percent: Option<f64>,
    pub scientific: bool,
    pub radix: bool,
}

pub fn parse_line(line: &str, prev: Option<&str>, vars: &HashSet<String>) -> Option<Program> {
    if line.len() > 4000 {
        return None;
    }
    let body = after_label(line);
    let tokens = lexer::tokenize(body, vars)?;
    if tokens.is_empty()
        || tokens
            .iter()
            .any(|t| matches!(t.kind, lexer::Kind::Aggregate))
    {
        return None;
    }

    let money = tokens.iter().any(|t| matches!(t.kind, lexer::Kind::Cur(_)));
    if money
        && tokens.windows(2).any(|pair| {
            matches!(pair[0].kind, lexer::Kind::Num { .. })
                && pair[1].glued
                && match &pair[1].kind {
                    lexer::Kind::Word(word) => !lexer::MONEY_SCALES
                        .iter()
                        .any(|(name, _)| word.eq_ignore_ascii_case(name)),
                    lexer::Kind::Unit(_) | lexer::Kind::CurWord(_, _) => true,
                    _ => false,
                }
        })
    {
        return None;
    }

    if tokens.iter().any(|t| match &t.kind {
        lexer::Kind::Word(word) => lexer::PROSE.iter().any(|p| word.eq_ignore_ascii_case(p)),
        _ => false,
    }) {
        return None;
    }

    let stmt = parse::Parser::new(&tokens, prev).statement()?;
    let (mut fend, scientific) = lower::lower(&stmt.expr);
    let radix = lower::uses_radix(&stmt.expr);
    let percent = match &stmt.expr {
        Ast::Pct(inner) => match inner.as_ref() {
            Ast::Num { value, .. } => Some(*value),
            _ => None,
        },
        _ => None,
    };
    if let Some(name) = stmt.name.as_deref() {
        match stmt.op {
            Some(AssignOp::Plain) | None => fend = format!("{name} = {fend}"),
            Some(op) => {
                let symbol = match op {
                    AssignOp::Add => "+",
                    AssignOp::Sub => "-",
                    AssignOp::Mul => "*",
                    AssignOp::Div => "/",
                    AssignOp::Plain => unreachable!(),
                };

                fend = if vars.contains(name) {
                    format!("{name} = ({name}) {symbol} {fend}")
                } else {
                    format!("{name} = {fend}")
                };
            }
        }
    }

    Some(Program {
        fend,
        name: stmt.name,
        percent,
        scientific,
        radix,
    })
}

fn after_label(line: &str) -> &str {
    for (at, c) in line.char_indices() {
        if c == ':' {
            let rest = &line[at + 1..];
            let spaced = rest.is_empty() || rest.starts_with(|c: char| c.is_whitespace());
            if spaced && !rest.trim().is_empty() && is_label(&line[..at]) {
                return rest.trim();
            }
            return line;
        }
        if !(c.is_alphabetic() || c.is_whitespace())
            && !matches!(c, '\'' | '-' | '_' | '&' | '/' | '.' | '’')
        {
            return line;
        }
    }
    line
}

fn is_label(left: &str) -> bool {
    !left.trim().is_empty() && left.chars().any(char::is_alphabetic)
}

pub fn scientific(raw: &str) -> String {
    lower::scientific(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lowered(line: &str) -> Option<String> {
        parse_line(line, None, &HashSet::new()).map(|program| program.fend)
    }

    #[test]
    fn word_operators_become_symbols() {
        assert_eq!(lowered("15 and 9").as_deref(), Some("(15) + (9)"));
        assert_eq!(lowered("5 with 3").as_deref(), Some("(5) + (3)"));
        assert_eq!(lowered("10 without 2").as_deref(), Some("(10) - (2)"));
        assert_eq!(lowered("12 x 10").as_deref(), Some("(12) * (10)"));
        assert_eq!(lowered("12 × 10").as_deref(), Some("(12) * (10)"));
        assert_eq!(lowered("7 ÷ 2").as_deref(), Some("(7) / (2)"));
        assert_eq!(lowered("20 divided by 4").as_deref(), Some("(20) / (4)"));
        assert_eq!(lowered("20 divide by 4").as_deref(), Some("(20) / (4)"));
        assert_eq!(lowered("6 mod 4").as_deref(), Some("(6) mod (4)"));
        assert_eq!(lowered("12 xor 10").as_deref(), Some("(12) xor (10)"));
        assert_eq!(lowered("1 << 8").as_deref(), Some("(1) << (8)"));
    }

    #[test]
    fn assignments_read_as_words_and_compounds() {
        assert_eq!(lowered("x = 5").as_deref(), Some("x = 5"));
        assert_eq!(lowered("x is 5").as_deref(), Some("x = 5"));
        assert_eq!(lowered("x equal 5").as_deref(), Some("x = 5"));
        let mut vars = HashSet::new();
        vars.insert("x".to_string());
        assert_eq!(
            parse_line("x += 5", None, &vars).map(|p| p.fend).as_deref(),
            Some("x = (x) + 5")
        );
        assert_eq!(
            parse_line("x /= 4", None, &vars).map(|p| p.fend).as_deref(),
            Some("x = (x) / 4")
        );

        assert_eq!(lowered("y += 5").as_deref(), Some("y = 5"));
    }

    #[test]
    fn percent_forms_cover_the_whole_table() {
        assert_eq!(lowered("20% of 100").as_deref(), Some("(20)% of (100)"));
        assert_eq!(lowered("20% from 100").as_deref(), Some("(20)% of (100)"));
        assert_eq!(
            lowered("5% on $30").as_deref(),
            Some("(30 USD) * (1 + (5)%)")
        );
        assert_eq!(lowered("6% off 40").as_deref(), Some("(40) * (1 - (6)%)"));
        assert_eq!(lowered("5 pct of 200").as_deref(), Some("(5)% of (200)"));
        assert_eq!(
            lowered("20% of what is 30").as_deref(),
            Some("(30) / ((20)%)")
        );
        assert_eq!(
            lowered("5% on what is 105").as_deref(),
            Some("(105) / (1 + (5)%)")
        );
        assert_eq!(
            lowered("5% off what is 95").as_deref(),
            Some("(95) / (1 - (5)%)")
        );
        assert_eq!(
            lowered("$100 + 5%").as_deref(),
            Some("(100 USD) * (1 + (5)%)")
        );
        assert_eq!(
            lowered("$100 - 5%").as_deref(),
            Some("(100 USD) * (1 - (5)%)")
        );
        assert_eq!(
            lowered("200 * 50%").as_deref(),
            Some("(200) * (((50) / 100))")
        );
        assert_eq!(
            lowered("$50 as a % of $100").as_deref(),
            Some("((50 USD) / (100 USD)) to %")
        );
        assert_eq!(
            lowered("120 as a % increase of 100").as_deref(),
            Some("(((120) / (100)) - 1) to %")
        );
        assert_eq!(
            lowered("80 as a percent decrease of 100").as_deref(),
            Some("(1 - ((80) / (100))) to %")
        );
    }

    #[test]
    fn scale_words_and_money_scales() {
        assert_eq!(lowered("5 thousand").as_deref(), Some("(5) * (1000)"));
        assert_eq!(lowered("2 million").as_deref(), Some("(2) * (1000000)"));
        assert_eq!(
            lowered("1.5 billion").as_deref(),
            Some("(1.5) * (1000000000)")
        );
        assert_eq!(
            lowered("2k + 3k").as_deref(),
            Some("((2) * (1000)) + ((3) * (1000))")
        );
        assert_eq!(lowered("$5m").as_deref(), Some("((5) * (1000000)) USD"));
        assert_eq!(lowered("$180K").as_deref(), Some("((180) * (1000)) USD"));
        assert_eq!(
            lowered("$2.5bn").as_deref(),
            Some("((2.5) * (1000000000)) USD")
        );

        assert_eq!(lowered("10K").as_deref(), Some("10 K"));
    }

    #[test]
    fn radix_numbers_and_notation_casts() {
        assert_eq!(lowered("0xFF").as_deref(), Some("0xff"));
        assert_eq!(lowered("0b1010").as_deref(), Some("0b1010"));
        assert_eq!(lowered("0o17").as_deref(), Some("0o17"));
        assert_eq!(lowered("255 in hex").as_deref(), Some("(255) in hex"));
        assert_eq!(lowered("10 in bin").as_deref(), Some("(10) in binary"));
        assert_eq!(lowered("10 in oct").as_deref(), Some("(10) in octal"));
        assert_eq!(lowered("0xFF in dec").as_deref(), Some("(0xff) in decimal"));
        let program = parse_line("5300 in sci", None, &HashSet::new()).unwrap();
        assert!(program.scientific);
        assert_eq!(program.fend, "(5300)");
    }

    #[test]
    fn numbers_keep_their_digits() {
        assert_eq!(
            lowered("12345678901234567890").as_deref(),
            Some("12345678901234567890")
        );
        assert_eq!(lowered("1,234.5").as_deref(), Some("1234.5"));
        assert_eq!(lowered("1 000").as_deref(), Some("1000"));
        assert_eq!(lowered(".5").as_deref(), Some(".5"));
        assert_eq!(lowered("2.5E-4").as_deref(), Some("2.5E-4"));
    }

    #[test]
    fn currency_works_both_ways_round() {
        assert_eq!(lowered("$10").as_deref(), Some("10 USD"));
        assert_eq!(lowered("10$").as_deref(), Some("10 USD"));
        assert_eq!(lowered("50 INR").as_deref(), Some("50 INR"));
        assert_eq!(lowered("INR 50").as_deref(), Some("50 INR"));
        assert_eq!(lowered("R$10").as_deref(), Some("10 BRL"));
        assert_eq!(lowered("Dh 100").as_deref(), Some("100 AED"));
        assert_eq!(lowered("SFr 20").as_deref(), Some("20 CHF"));
        assert_eq!(
            lowered("100 USD in EUR").as_deref(),
            Some("(100 USD) in EUR")
        );
    }

    #[test]
    fn functions_take_parens_spaces_and_words() {
        assert_eq!(lowered("sqrt(16)").as_deref(), Some("sqrt(16)"));
        assert_eq!(lowered("sqrt 16").as_deref(), Some("sqrt(16)"));
        assert_eq!(lowered("square root of 16").as_deref(), Some("sqrt(16)"));
        assert_eq!(lowered("cube root 27").as_deref(), Some("cbrt(27)"));
        assert_eq!(lowered("fact 5").as_deref(), Some("(5)!"));
        assert_eq!(lowered("5!").as_deref(), Some("(5)!"));
        assert_eq!(lowered("arcsin 0.5").as_deref(), Some("asin(0.5)"));
        assert_eq!(lowered("sqrt 4 + 12").as_deref(), Some("(sqrt(4)) + (12)"));
        assert_eq!(lowered("sqrt(4 + 12)").as_deref(), Some("sqrt((4) + (12))"));
    }

    #[test]
    fn units_are_phrases_not_patterns() {
        assert_eq!(
            lowered("1 nautical mile in km").as_deref(),
            Some("(1 nautical_mile) in km")
        );
        assert_eq!(lowered("8 fl oz in ml").as_deref(), Some("(8 floz) in ml"));
        assert_eq!(lowered("120 sq ft").as_deref(), Some("120 ft^2"));
        assert_eq!(
            lowered("3 ft + 2 in").as_deref(),
            Some("(3 ft) + (2 inches)")
        );
        assert_eq!(lowered("10 km/h").as_deref(), Some("10 km/h"));
        assert_eq!(
            lowered("2 hours into minutes").as_deref(),
            Some("(2 hours) in minutes")
        );
    }

    #[test]
    fn a_lowercase_megabit_is_not_a_millibarn() {
        assert_eq!(lowered("1 mb to kb").as_deref(), Some("(1 megabit) in kb"));
        assert_eq!(lowered("5mb in kb").as_deref(), Some("(5 megabit) in kb"));
        assert_eq!(lowered("1 MB in KB").as_deref(), Some("(1 MB) in KB"));
        assert_eq!(
            lowered("1 megabyte in mb").as_deref(),
            Some("(1 megabyte) in megabit")
        );
    }

    #[test]
    fn prose_and_odd_shapes_are_left_to_the_other_path() {
        assert_eq!(lowered("4 GB at 100 Mbps"), None);
        assert_eq!(lowered("total"), None);
        assert_eq!(lowered("20 liters in imperial gallons"), None);
        assert_eq!(lowered("Movie tickets 2 * $14"), None);
        assert_eq!(lowered("$84 split 3 ways"), None);
        assert_eq!(lowered("18% tip on $62"), None);
        assert_eq!(lowered("now").as_deref(), Some("now"));
        assert_eq!(lowered("Price: $10").as_deref(), Some("10 USD"));
        assert_eq!(lowered("15:30").as_deref(), None);
    }
}
