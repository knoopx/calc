use super::lexer::{BinOp, Dir, Func, Kind, Repr, Token};

#[derive(Clone, Debug, PartialEq)]
pub enum Ast {
    Num {
        text: String,
        value: f64,
    },
    Base {
        value: f64,
        repr: Repr,
    },
    Unit {
        expr: Box<Ast>,
        unit: String,
    },
    Cur {
        expr: Box<Ast>,
        code: String,
    },
    Var(String),
    Bin {
        op: BinOp,
        lhs: Box<Ast>,
        rhs: Box<Ast>,
    },
    Neg(Box<Ast>),
    Call {
        func: Func,
        arg: Box<Ast>,
    },
    Fact(Box<Ast>),
    Pct(Box<Ast>),
    PctDir {
        dir: Dir,
        pct: Box<Ast>,
        base: Box<Ast>,
    },
    RevPct {
        dir: Dir,
        pct: Box<Ast>,
        result: Box<Ast>,
    },
    InlinePct {
        add: bool,
        base: Box<Ast>,
        pct: Box<Ast>,
    },
    Ratio(Box<Ast>),
    AsPct {
        dir: Dir,
        value: Box<Ast>,
        base: Box<Ast>,
    },
    Convert {
        expr: Box<Ast>,
        target: Target,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Unit(String),
    Cur(String),
    Repr(Repr),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignOp {
    Plain,
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    pub name: Option<String>,
    pub op: Option<AssignOp>,
    pub expr: Ast,
}

const BP_CONVERT: u8 = 1;
const BP_PCT_ASR: u8 = 3;
const BP_BIT: u8 = 5;
const BP_SHIFT: u8 = 7;
const BP_ADD: u8 = 9;
const BP_MUL: u8 = 11;
const BP_POW: u8 = 13;
const BP_UNARY: u8 = 15;
const BP_FUNC: u8 = 17;
const BP_CURRENCY: u8 = 19;

pub struct Parser<'a> {
    tokens: &'a [Token],
    at: usize,
    prev: Option<&'a str>,
    currency_prefix: bool,
}

impl<'a> Parser<'a> {
    pub fn new(tokens: &'a [Token], prev: Option<&'a str>) -> Self {
        Self {
            tokens,
            at: 0,
            prev,
            currency_prefix: false,
        }
    }

    fn peek(&self) -> &Kind {
        self.tokens
            .get(self.at)
            .map(|t| &t.kind)
            .unwrap_or(&Kind::Eof)
    }

    fn peek_at(&self, ahead: usize) -> &Kind {
        self.tokens
            .get(self.at + ahead)
            .map(|t| &t.kind)
            .unwrap_or(&Kind::Eof)
    }

    fn take(&mut self) -> Kind {
        let kind = self.peek().clone();
        if self.at < self.tokens.len() {
            self.at += 1;
        }
        kind
    }

    pub fn statement(&mut self) -> Option<Stmt> {
        if let Kind::Word(name) = self.peek() {
            let name = name.clone();
            let op = match self.peek_at(1) {
                Kind::Assign => Some(AssignOp::Plain),
                Kind::PlusEq => Some(AssignOp::Add),
                Kind::MinusEq => Some(AssignOp::Sub),
                Kind::MulEq => Some(AssignOp::Mul),
                Kind::DivEq => Some(AssignOp::Div),
                _ => None,
            };
            if let Some(op) = op {
                self.at += 2;
                let expr = self.expression(0)?;
                if *self.peek() != Kind::Eof {
                    return None;
                }
                return Some(Stmt {
                    name: Some(name),
                    op: Some(op),
                    expr,
                });
            }
        }
        let expr = self.expression(0)?;
        if *self.peek() != Kind::Eof {
            return None;
        }
        Some(Stmt {
            name: None,
            op: None,
            expr,
        })
    }

    fn expression(&mut self, min_bp: u8) -> Option<Ast> {
        let mut lhs = self.prefix()?;
        while let Some((l_bp, r_bp)) = self.infix_bp(self.peek()) {
            if l_bp < min_bp {
                break;
            }
            lhs = self.infix(lhs, r_bp)?;
        }
        Some(lhs)
    }

    fn infix_bp(&self, kind: &Kind) -> Option<(u8, u8)> {
        match kind {
            Kind::In | Kind::Convert | Kind::From => Some((BP_CONVERT, BP_CONVERT + 1)),
            Kind::AsPct(_) => Some((BP_PCT_ASR, BP_PCT_ASR + 1)),
            Kind::Op(BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor) => Some((BP_BIT, BP_BIT + 1)),
            Kind::Op(BinOp::Shl | BinOp::Shr) => Some((BP_SHIFT, BP_SHIFT + 1)),
            Kind::Op(BinOp::Add | BinOp::Sub) => Some((BP_ADD, BP_ADD + 1)),
            Kind::Op(BinOp::Mul | BinOp::Div | BinOp::Mod) => Some((BP_MUL, BP_MUL + 1)),

            Kind::Op(BinOp::Pow) => Some((BP_POW, BP_POW)),
            _ => None,
        }
    }

    fn prefix(&mut self) -> Option<Ast> {
        match self.take() {
            Kind::Num { text, value } => self.postfix(Ast::Num { text, value }),
            Kind::Base { value, repr } => self.postfix(Ast::Base { value, repr }),
            Kind::LParen => {
                let inner = self.expression(0)?;
                if *self.peek() != Kind::RParen {
                    return None;
                }
                self.take();
                self.postfix(inner)
            }
            Kind::Op(BinOp::Sub) => Some(Ast::Neg(Box::new(self.expression(BP_UNARY)?))),
            Kind::Op(BinOp::Add) => self.expression(BP_UNARY),
            Kind::Func(func) => {
                let arg = if *self.peek() == Kind::LParen {
                    self.take();
                    let inner = self.expression(0)?;
                    if *self.peek() != Kind::RParen {
                        return None;
                    }
                    self.take();
                    inner
                } else {
                    if matches!(self.peek(), Kind::Dir(Dir::Of)) {
                        self.take();
                    }
                    self.expression(BP_FUNC)?
                };
                Some(Ast::Call {
                    func,
                    arg: Box::new(arg),
                })
            }
            Kind::Cur(code) => {
                self.currency_prefix = true;
                let inner = self.expression(BP_CURRENCY);
                self.currency_prefix = false;
                Some(Ast::Cur {
                    expr: Box::new(inner?),
                    code: code.to_string(),
                })
            }
            Kind::CurWord(code, _) if self.starts_value() => {
                self.currency_prefix = true;
                let inner = self.expression(BP_CURRENCY);
                self.currency_prefix = false;
                Some(Ast::Cur {
                    expr: Box::new(inner?),
                    code: code.to_string(),
                })
            }
            Kind::CurWord(_, word) => Some(Ast::Var(word)),
            Kind::Word(word) => {
                if word == "prev" {
                    return self.prev.map(|p| Ast::Var(p.to_string()));
                }

                if word.len() == 3
                    && word.chars().all(|c| c.is_ascii_uppercase())
                    && super::lexer::CURRENCY_CODES
                        .iter()
                        .any(|code| code.eq_ignore_ascii_case(word.as_str()))
                    && self.starts_value()
                {
                    self.currency_prefix = true;
                    let inner = self.expression(BP_CURRENCY);
                    self.currency_prefix = false;
                    return Some(Ast::Cur {
                        expr: Box::new(inner?),
                        code: word,
                    });
                }
                Some(Ast::Var(word))
            }
            _ => None,
        }
    }

    fn starts_value(&self) -> bool {
        self.starts_value_at(0)
    }

    fn starts_value_at(&self, ahead: usize) -> bool {
        matches!(
            self.peek_at(ahead),
            Kind::Num { .. }
                | Kind::Base { .. }
                | Kind::LParen
                | Kind::Op(BinOp::Sub)
                | Kind::Cur(_)
        )
    }

    fn postfix(&mut self, node: Ast) -> Option<Ast> {
        if *self.peek() == Kind::Pct {
            self.take();
            return self.percent(node);
        }

        let mut node = node;
        if let Kind::Scale(factor) = self.peek() {
            let factor = *factor;
            self.take();
            node = Ast::Bin {
                op: BinOp::Mul,
                lhs: Box::new(node),
                rhs: Box::new(Ast::Num {
                    text: amount(factor),
                    value: factor,
                }),
            };
        }

        if let Kind::Word(word) = self.peek()
            && let Some(factor) = money_scale(word)
            && (self.currency_prefix || has_currency(&node))
        {
            self.take();
            node = Ast::Bin {
                op: BinOp::Mul,
                lhs: Box::new(node),
                rhs: Box::new(Ast::Num {
                    text: amount(factor),
                    value: factor,
                }),
            };
        }

        let unit = self.unit_phrase(&node)?;
        let node = match unit {
            Some(unit) => Ast::Unit {
                expr: Box::new(node),
                unit,
            },
            None => node,
        };

        let node = match self.peek() {
            Kind::Cur(code) => {
                let code = *code;
                self.take();
                Ast::Cur {
                    expr: Box::new(node),
                    code: code.to_string(),
                }
            }
            Kind::CurWord(code, _) => {
                let code = *code;
                self.take();
                Ast::Cur {
                    expr: Box::new(node),
                    code: code.to_string(),
                }
            }
            _ => node,
        };

        Some(match self.peek() {
            Kind::Bang => {
                self.take();
                Ast::Fact(Box::new(node))
            }
            _ => node,
        })
    }

    fn unit_phrase(&mut self, node: &Ast) -> Option<Option<String>> {
        let atomic = matches!(node, Ast::Num { .. } | Ast::Base { .. });
        let mut parts: Vec<String> = Vec::new();
        loop {
            match self.peek() {
                Kind::Word(word) => parts.push(word.clone()),
                Kind::Unit(unit) => parts.push((*unit).to_string()),
                Kind::In
                    if parts.is_empty() && atomic && !self.conversion_target(self.peek_at(1)) =>
                {
                    self.take();
                    parts.push("inches".to_string());
                }
                _ => break,
            }
            self.take();
        }
        if parts.is_empty() {
            return Some(None);
        }

        if *self.peek() == Kind::Op(BinOp::Div) {
            if let Kind::Word(next) = self.peek_at(1) {
                let next = next.clone();
                self.at += 2;
                parts.push(format!("/{next}"));
            } else if let Kind::Unit(next) = self.peek_at(1) {
                let next = *next;
                self.at += 2;
                parts.push(format!("/{next}"));
            }
        }

        let mut joined = String::new();
        for part in parts {
            if part.starts_with('/') {
                joined.push_str(&part);
            } else {
                if !joined.is_empty() {
                    joined.push(' ');
                }
                joined.push_str(&part);
            }
        }
        Some(Some(joined))
    }

    fn conversion_target(&self, kind: &Kind) -> bool {
        matches!(
            kind,
            Kind::Word(_) | Kind::Unit(_) | Kind::Repr(_) | Kind::Cur(_) | Kind::CurWord(_, _)
        )
    }

    fn percent(&mut self, pct: Ast) -> Option<Ast> {
        match self.peek() {
            Kind::Dir(dir) => {
                let dir = *dir;
                self.take();
                self.percent_dir(dir, pct)
            }
            Kind::From => {
                self.take();
                self.percent_dir(Dir::Of, pct)
            }
            Kind::Rev(dir) => {
                let dir = *dir;
                self.take();
                let result = self.expression(0)?;
                Some(Ast::RevPct {
                    dir,
                    pct: Box::new(pct),
                    result: Box::new(result),
                })
            }
            _ => Some(Ast::Pct(Box::new(pct))),
        }
    }

    fn percent_dir(&mut self, dir: Dir, pct: Ast) -> Option<Ast> {
        let base = self.expression(0)?;
        Some(Ast::PctDir {
            dir,
            pct: Box::new(pct),
            base: Box::new(base),
        })
    }

    fn infix(&mut self, lhs: Ast, r_bp: u8) -> Option<Ast> {
        match self.take() {
            Kind::Op(op) => {
                let rhs = self.expression(r_bp)?;
                let percent = match &rhs {
                    Ast::Pct(pct) => Some(pct.clone()),
                    _ => None,
                };
                if let Some(pct) = percent {
                    match op {
                        BinOp::Add | BinOp::Sub => {
                            return Some(Ast::InlinePct {
                                add: op == BinOp::Add,
                                base: Box::new(lhs),
                                pct,
                            });
                        }

                        BinOp::Mul | BinOp::Div => {
                            return Some(Ast::Bin {
                                op,
                                lhs: Box::new(lhs),
                                rhs: Box::new(Ast::Ratio(pct)),
                            });
                        }
                        _ => {}
                    }
                }
                Some(Ast::Bin {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                })
            }
            kind @ (Kind::In | Kind::Convert | Kind::From) => {
                let target = match self.peek().clone() {
                    Kind::Repr(repr) => {
                        self.take();
                        Target::Repr(repr)
                    }
                    Kind::Cur(code) => {
                        self.take();
                        Target::Cur(code.to_string())
                    }
                    Kind::CurWord(code, _) => {
                        self.take();
                        Target::Cur(code.to_string())
                    }
                    Kind::Word(_) | Kind::Unit(_) | Kind::In => {
                        let mut parts: Vec<String> = Vec::new();
                        while matches!(self.peek(), Kind::Word(_) | Kind::Unit(_)) {
                            match self.take() {
                                Kind::Word(word) => parts.push(word),
                                Kind::Unit(unit) => parts.push(unit.to_string()),
                                Kind::In => parts.push("inches".to_string()),
                                _ => break,
                            }
                        }
                        if parts.is_empty() {
                            return None;
                        }
                        Target::Unit(parts.join(" "))
                    }
                    _ => return None,
                };
                let _ = kind;
                Some(Ast::Convert {
                    expr: Box::new(lhs),
                    target,
                })
            }
            Kind::AsPct(dir) => {
                let value = self.expression(r_bp)?;
                Some(Ast::AsPct {
                    dir,
                    value: Box::new(lhs),
                    base: Box::new(value),
                })
            }
            _ => None,
        }
    }
}

fn money_scale(word: &str) -> Option<f64> {
    super::lexer::MONEY_SCALES
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(word))
        .map(|(_, factor)| *factor)
}

fn amount(factor: f64) -> String {
    if factor == factor.trunc() && factor < 1e24 {
        format!("{}", factor as i128)
    } else {
        format!("{factor}")
    }
}

fn has_currency(ast: &Ast) -> bool {
    match ast {
        Ast::Cur { .. } => true,
        Ast::Unit { expr, .. } => has_currency(expr),
        _ => false,
    }
}
