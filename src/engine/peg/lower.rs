use super::lexer::{BinOp, Dir, Func, Repr};
use super::parse::{Ast, Target};

pub fn lower(ast: &Ast) -> (String, bool) {
    let mut scientific = false;
    (text(ast, &mut scientific), scientific)
}

pub fn uses_radix(ast: &Ast) -> bool {
    match ast {
        Ast::Base { .. } => true,
        Ast::Unit { expr, .. } | Ast::Cur { expr, .. } => uses_radix(expr),
        Ast::Neg(inner) | Ast::Fact(inner) | Ast::Pct(inner) | Ast::Ratio(inner) => {
            uses_radix(inner)
        }
        Ast::Call { arg, .. } => uses_radix(arg),
        Ast::Bin { lhs, rhs, .. } => uses_radix(lhs) || uses_radix(rhs),
        Ast::PctDir { pct, base, .. } => uses_radix(pct) || uses_radix(base),
        Ast::RevPct { pct, result, .. } => uses_radix(pct) || uses_radix(result),
        Ast::InlinePct { base, pct, .. } => uses_radix(base) || uses_radix(pct),
        Ast::AsPct { value, base, .. } => uses_radix(value) || uses_radix(base),
        Ast::Convert { expr, target } => matches!(target, Target::Repr(_)) || uses_radix(expr),
        Ast::Num { .. } | Ast::Var(_) => false,
    }
}

fn text(ast: &Ast, scientific: &mut bool) -> String {
    match ast {
        Ast::Num { text, value } => {
            if text.chars().all(|c| {
                c.is_ascii_digit() || c == '.' || c == '-' || c == 'e' || c == 'E' || c == '+'
            }) {
                text.clone()
            } else {
                number(*value)
            }
        }
        Ast::Base { value, repr } => match repr {
            Repr::Hex => format!("0x{:x}", value.round() as i64),
            Repr::Binary => format!("0b{}", binary(*value)),
            Repr::Octal => format!("0o{:o}", value.round() as i64),
            _ => number(*value),
        },
        Ast::Var(name) => name.clone(),
        Ast::Unit { expr, unit } => {
            let inner = text(expr, scientific);
            if atomic(expr) {
                format!("{inner} {unit}")
            } else {
                format!("({inner}) {unit}")
            }
        }
        Ast::Cur { expr, code } => {
            let inner = text(expr, scientific);
            if atomic(expr) {
                format!("{inner} {code}")
            } else {
                format!("({inner}) {code}")
            }
        }
        Ast::Bin { op, lhs, rhs } => {
            if op == &BinOp::Div
                && let Ast::Unit { expr, unit } = rhs.as_ref()
                && !measured(lhs)
                && !measured(expr)
            {
                let inner = text(lhs, scientific);
                let value = text(expr, scientific);
                return format!("({inner} / {value}) {unit}");
            }
            let op = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "/",
                BinOp::Pow => "^",
                BinOp::Mod => "mod",
                BinOp::BitAnd => "&",
                BinOp::BitOr => "|",
                BinOp::BitXor => "xor",
                BinOp::Shl => "<<",
                BinOp::Shr => ">>",
            };
            format!(
                "({}) {op} ({})",
                text(lhs, scientific),
                text(rhs, scientific)
            )
        }
        Ast::Neg(inner) => format!("-({})", text(inner, scientific)),
        Ast::Call { func, arg } => {
            let arg = text(arg, scientific);
            match func {
                Func::Fact => format!("({arg})!"),
                other => format!("{}({arg})", name(other)),
            }
        }
        Ast::Fact(inner) => format!("({})!", text(inner, scientific)),
        Ast::Pct(inner) => format!("({})%", text(inner, scientific)),
        Ast::Ratio(inner) => format!("(({}) / 100)", text(inner, scientific)),
        Ast::PctDir { dir, pct, base } => {
            let pct = text(pct, scientific);
            let base = text(base, scientific);
            match dir {
                Dir::Of => format!("({pct})% of ({base})"),
                Dir::On => format!("({base}) * (1 + ({pct})%)"),
                Dir::Off => format!("({base}) * (1 - ({pct})%)"),
            }
        }
        Ast::RevPct { dir, pct, result } => {
            let pct = text(pct, scientific);
            let result = text(result, scientific);
            match dir {
                Dir::Of => format!("({result}) / (({pct})%)"),
                Dir::On => format!("({result}) / (1 + ({pct})%)"),
                Dir::Off => format!("({result}) / (1 - ({pct})%)"),
            }
        }
        Ast::InlinePct { add, base, pct } => {
            let base = text(base, scientific);
            let pct = text(pct, scientific);
            let sign = if *add { "+" } else { "-" };
            format!("({base}) * (1 {sign} ({pct})%)")
        }
        Ast::AsPct { dir, value, base } => {
            let value = text(value, scientific);
            let base = text(base, scientific);
            let share = match dir {
                Dir::Of => format!("(({value}) / ({base}))"),
                Dir::On => format!("((({value}) / ({base})) - 1)"),
                Dir::Off => format!("(1 - (({value}) / ({base})))"),
            };
            format!("{share} to %")
        }
        Ast::Convert { expr, target } => {
            let inner = text(expr, scientific);
            match target {
                Target::Unit(unit) => format!("({inner}) in {unit}"),
                Target::Cur(code) => format!("({inner}) in {code}"),
                Target::Repr(Repr::Scientific) => {
                    *scientific = true;
                    format!("({inner})")
                }
                Target::Repr(repr) => format!("({inner}) in {}", repr_word(repr)),
            }
        }
    }
}

fn measured(ast: &Ast) -> bool {
    match ast {
        Ast::Unit { .. } | Ast::Cur { .. } => true,
        Ast::Bin { lhs, rhs, .. } => measured(lhs) || measured(rhs),
        Ast::Neg(inner) | Ast::Fact(inner) | Ast::Pct(inner) | Ast::Ratio(inner) => measured(inner),
        Ast::Call { arg, .. } => measured(arg),
        Ast::Convert { expr, .. } => measured(expr),
        Ast::PctDir { pct, base, .. } => measured(pct) || measured(base),
        Ast::RevPct { pct, result, .. } => measured(pct) || measured(result),
        Ast::InlinePct { base, pct, .. } => measured(base) || measured(pct),
        Ast::AsPct { value, base, .. } => measured(value) || measured(base),
        _ => false,
    }
}

fn atomic(ast: &Ast) -> bool {
    matches!(ast, Ast::Num { .. } | Ast::Base { .. } | Ast::Var(_))
}

fn repr_word(repr: &Repr) -> &'static str {
    match repr {
        Repr::Hex => "hex",
        Repr::Binary => "binary",
        Repr::Octal => "octal",
        Repr::Decimal => "decimal",
        Repr::Scientific => "scientific",
    }
}

fn name(func: &Func) -> &'static str {
    match func {
        Func::Sqrt => "sqrt",
        Func::Cbrt => "cbrt",
        Func::Abs => "abs",
        Func::Round => "round",
        Func::Ceil => "ceil",
        Func::Floor => "floor",
        Func::Ln => "ln",
        Func::Log => "log",
        Func::Fact => "fact",
        Func::Sin => "sin",
        Func::Cos => "cos",
        Func::Tan => "tan",
        Func::Asin => "asin",
        Func::Acos => "acos",
        Func::Atan => "atan",
        Func::Sinh => "sinh",
        Func::Cosh => "cosh",
        Func::Tanh => "tanh",
    }
}

fn number(value: f64) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    let absolute = value.abs();
    if absolute != 0.0 && (absolute >= 1e16 || absolute < 1e-9) {
        return format!("{value:e}");
    }
    let text = format!("{value}");
    match text.strip_suffix(".0") {
        Some(trimmed) => trimmed.to_string(),
        None => text,
    }
}

fn binary(value: f64) -> String {
    let mut bits = String::new();
    let mut rest = value.floor();
    if rest == 0.0 {
        return "0".to_string();
    }
    while rest > 0.0 {
        bits.push(if rest % 2.0 >= 1.0 { '1' } else { '0' });
        rest = (rest / 2.0).floor();
    }
    bits.chars().rev().collect()
}

pub fn scientific(raw: &str) -> String {
    let raw = raw.trim_start_matches("approx. ");
    let cut = raw
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')))
        .map(|(at, _)| at)
        .unwrap_or(raw.len());
    let (head, tail) = raw.split_at(cut);
    let Ok(value) = head.parse::<f64>() else {
        return raw.to_string();
    };
    if value == 0.0 {
        return format!("0.00e0{tail}");
    }
    format!("{value:.2e}{tail}")
}
