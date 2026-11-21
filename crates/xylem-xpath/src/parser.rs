//! Recursive-descent parser for the XPath 1.0 grammar.

use crate::ast::{Axis, BinOp, Expr, NodeTest, PathExpr, PathStart, Step};
use crate::error::XPathError;
use crate::lexer::{Spanned, Token, tokenize};

const MAX_DEPTH: usize = 64;

pub(crate) fn parse(src: &str) -> Result<Expr, XPathError> {
    let tokens = tokenize(src)?;
    let end = src.chars().count();
    let mut p = Parser { tokens, idx: 0, end, depth: 0 };
    let expr = p.or_expr()?;
    if let Some(t) = p.tokens.get(p.idx) {
        return Err(XPathError::syntax(t.pos, format!("unexpected token {:?}", t.token)));
    }
    Ok(expr)
}

struct Parser {
    tokens: Vec<Spanned>,
    idx: usize,
    end: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.idx).map(|s| &s.token)
    }

    fn pos(&self) -> usize {
        self.tokens.get(self.idx).map_or(self.end, |s| s.pos)
    }

    fn bump(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.idx).map(|s| s.token.clone());
        if t.is_some() {
            self.idx += 1;
        }
        t
    }

    fn eat(&mut self, token: &Token) -> bool {
        if self.peek() == Some(token) {
            self.idx += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, token: &Token, what: &str) -> Result<(), XPathError> {
        if self.eat(token) { Ok(()) } else { Err(XPathError::syntax(self.pos(), format!("expected {what}"))) }
    }

    fn enter(&mut self) -> Result<(), XPathError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(XPathError::syntax(self.pos(), "expression is nested too deeply"));
        }
        Ok(())
    }

    fn or_expr(&mut self) -> Result<Expr, XPathError> {
        self.enter()?;
        let mut lhs = self.and_expr()?;
        while self.eat(&Token::Or) {
            lhs = Expr::Or(Box::new(lhs), Box::new(self.and_expr()?));
        }
        self.depth -= 1;
        Ok(lhs)
    }

    fn and_expr(&mut self) -> Result<Expr, XPathError> {
        let mut lhs = self.equality()?;
        while self.eat(&Token::And) {
            lhs = Expr::And(Box::new(lhs), Box::new(self.equality()?));
        }
        Ok(lhs)
    }

    fn equality(&mut self) -> Result<Expr, XPathError> {
        let mut lhs = self.relational()?;
        loop {
            let op = match self.peek() {
                Some(Token::Eq) => BinOp::Eq,
                Some(Token::Neq) => BinOp::Neq,
                _ => break,
            };
            self.idx += 1;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(self.relational()?));
        }
        Ok(lhs)
    }

    fn relational(&mut self) -> Result<Expr, XPathError> {
        let mut lhs = self.additive()?;
        loop {
            let op = match self.peek() {
                Some(Token::Lt) => BinOp::Lt,
                Some(Token::Le) => BinOp::Le,
                Some(Token::Gt) => BinOp::Gt,
                Some(Token::Ge) => BinOp::Ge,
                _ => break,
            };
            self.idx += 1;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(self.additive()?));
        }
        Ok(lhs)
    }

    fn additive(&mut self) -> Result<Expr, XPathError> {
        let mut lhs = self.multiplicative()?;
        loop {
            let op = match self.peek() {
                Some(Token::Plus) => BinOp::Add,
                Some(Token::Minus) => BinOp::Sub,
                _ => break,
            };
            self.idx += 1;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(self.multiplicative()?));
        }
        Ok(lhs)
    }

    fn multiplicative(&mut self) -> Result<Expr, XPathError> {
        let mut lhs = self.unary()?;
        loop {
            let op = match self.peek() {
                Some(Token::Multiply) => BinOp::Mul,
                Some(Token::Div) => BinOp::Div,
                Some(Token::Mod) => BinOp::Mod,
                _ => break,
            };
            self.idx += 1;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(self.unary()?));
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<Expr, XPathError> {
        if self.eat(&Token::Minus) {
            self.enter()?;
            let inner = self.unary()?;
            self.depth -= 1;
            return Ok(Expr::Neg(Box::new(inner)));
        }
        self.union()
    }

    fn union(&mut self) -> Result<Expr, XPathError> {
        let mut lhs = self.path_expr()?;
        while self.eat(&Token::Pipe) {
            lhs = Expr::Union(Box::new(lhs), Box::new(self.path_expr()?));
        }
        Ok(lhs)
    }

    fn starts_step(&self) -> bool {
        matches!(
            self.peek(),
            Some(
                Token::Name(_)
                    | Token::Star
                    | Token::PrefixStar(_)
                    | Token::NodeType(_)
                    | Token::AxisName(_)
                    | Token::At
                    | Token::Dot
                    | Token::DotDot
            )
        )
    }

    fn path_expr(&mut self) -> Result<Expr, XPathError> {
        match self.peek() {
            Some(Token::Slash) => {
                self.idx += 1;
                let mut steps = Vec::new();
                if self.starts_step() {
                    self.relative_steps(&mut steps)?;
                }
                Ok(Expr::Path(PathExpr { start: PathStart::Root, steps }))
            }
            Some(Token::DoubleSlash) => {
                self.idx += 1;
                let mut steps = Vec::new();
                self.descendant_step(&mut steps)?;
                self.continue_steps(&mut steps)?;
                Ok(Expr::Path(PathExpr { start: PathStart::Root, steps }))
            }
            Some(
                Token::Variable(_) | Token::LParen | Token::Literal(_) | Token::Number(_) | Token::FunctionName(_),
            ) => {
                let primary = self.primary()?;
                let mut predicates = Vec::new();
                while self.peek() == Some(&Token::LBracket) {
                    predicates.push(self.predicate()?);
                }
                let base = if predicates.is_empty() { primary } else { Expr::Filter(Box::new(primary), predicates) };
                let mut steps = Vec::new();
                match self.peek() {
                    Some(Token::Slash) => {
                        self.idx += 1;
                        self.relative_steps(&mut steps)?;
                    }
                    Some(Token::DoubleSlash) => {
                        self.idx += 1;
                        self.descendant_step(&mut steps)?;
                        self.continue_steps(&mut steps)?;
                    }
                    _ => return Ok(base),
                }
                Ok(Expr::Path(PathExpr { start: PathStart::Expr(Box::new(base)), steps }))
            }
            _ if self.starts_step() => {
                let mut steps = Vec::new();
                self.relative_steps(&mut steps)?;
                Ok(Expr::Path(PathExpr { start: PathStart::Context, steps }))
            }
            Some(t) => {
                let msg = format!("unexpected token {t:?}");
                Err(XPathError::syntax(self.pos(), msg))
            }
            None => Err(XPathError::syntax(self.pos(), "unexpected end of expression")),
        }
    }

    /// Parses `step (('/' | '//') step)*`.
    fn relative_steps(&mut self, steps: &mut Vec<Step>) -> Result<(), XPathError> {
        steps.push(self.step()?);
        self.continue_steps(steps)
    }

    fn continue_steps(&mut self, steps: &mut Vec<Step>) -> Result<(), XPathError> {
        loop {
            match self.peek() {
                Some(Token::Slash) => {
                    self.idx += 1;
                    steps.push(self.step()?);
                }
                Some(Token::DoubleSlash) => {
                    self.idx += 1;
                    self.descendant_step(steps)?;
                }
                _ => return Ok(()),
            }
        }
    }

    /// Handles what follows a `//`: pushes either a fused `descendant::` step or the
    /// expanded `descendant-or-self::node()/step` pair.
    fn descendant_step(&mut self, steps: &mut Vec<Step>) -> Result<(), XPathError> {
        let step = self.step()?;
        if step.axis == Axis::Child && step.predicates.is_empty() {
            steps.push(Step { axis: Axis::Descendant, test: step.test, predicates: step.predicates });
        } else {
            steps.push(Step { axis: Axis::DescendantOrSelf, test: NodeTest::Node, predicates: Vec::new() });
            steps.push(step);
        }
        Ok(())
    }

    fn step(&mut self) -> Result<Step, XPathError> {
        let pos = self.pos();
        match self.peek() {
            Some(Token::Dot) => {
                self.idx += 1;
                return Ok(Step { axis: Axis::Current, test: NodeTest::Node, predicates: Vec::new() });
            }
            Some(Token::DotDot) => {
                self.idx += 1;
                return Ok(Step { axis: Axis::Parent, test: NodeTest::Node, predicates: Vec::new() });
            }
            _ => {}
        }
        let axis = match self.peek() {
            Some(Token::At) => {
                self.idx += 1;
                Axis::Attribute
            }
            Some(Token::AxisName(name)) => {
                let axis = Axis::from_name(name)
                    .ok_or_else(|| XPathError::syntax(pos, format!("unknown axis '{name}'")))?;
                self.idx += 1;
                self.expect(&Token::ColonColon, "'::'")?;
                axis
            }
            _ => Axis::Child,
        };
        let test = self.node_test()?;
        let mut predicates = Vec::new();
        while self.peek() == Some(&Token::LBracket) {
            predicates.push(self.predicate()?);
        }
        Ok(Step { axis, test, predicates })
    }

    fn node_test(&mut self) -> Result<NodeTest, XPathError> {
        let pos = self.pos();
        match self.bump() {
            Some(Token::Name(n)) => Ok(NodeTest::Name(n)),
            Some(Token::Star) => Ok(NodeTest::Any),
            Some(Token::PrefixStar(p)) => Ok(NodeTest::PrefixWildcard(p)),
            Some(Token::NodeType(kind)) => {
                self.expect(&Token::LParen, "'('")?;
                let test = match kind.as_str() {
                    "node" => NodeTest::Node,
                    "text" => NodeTest::Text,
                    "comment" => NodeTest::Comment,
                    _ => {
                        if let Some(Token::Literal(lit)) = self.peek().cloned() {
                            self.idx += 1;
                            NodeTest::Pi(Some(lit))
                        } else {
                            NodeTest::Pi(None)
                        }
                    }
                };
                self.expect(&Token::RParen, "')'")?;
                Ok(test)
            }
            _ => Err(XPathError::syntax(pos, "expected a node test")),
        }
    }

    fn predicate(&mut self) -> Result<Expr, XPathError> {
        self.expect(&Token::LBracket, "'['")?;
        let e = self.or_expr()?;
        self.expect(&Token::RBracket, "']'")?;
        Ok(e)
    }

    fn primary(&mut self) -> Result<Expr, XPathError> {
        let pos = self.pos();
        match self.bump() {
            Some(Token::Variable(v)) => Ok(Expr::Variable(v)),
            Some(Token::Literal(s)) => Ok(Expr::Literal(s)),
            Some(Token::Number(n)) => Ok(Expr::Number(n)),
            Some(Token::LParen) => {
                let e = self.or_expr()?;
                self.expect(&Token::RParen, "')'")?;
                Ok(e)
            }
            Some(Token::FunctionName(name)) => {
                self.expect(&Token::LParen, "'('")?;
                let mut args = Vec::new();
                if self.peek() != Some(&Token::RParen) {
                    loop {
                        args.push(self.or_expr()?);
                        if !self.eat(&Token::Comma) {
                            break;
                        }
                    }
                }
                self.expect(&Token::RParen, "')'")?;
                Ok(Expr::Function(name, args))
            }
            _ => Err(XPathError::syntax(pos, "expected a primary expression")),
        }
    }
}
