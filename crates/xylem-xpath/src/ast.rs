//! Abstract syntax tree of a compiled expression.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Axis {
    Ancestor,
    AncestorOrSelf,
    Attribute,
    Child,
    Descendant,
    DescendantOrSelf,
    Following,
    FollowingSibling,
    Namespace,
    Parent,
    Preceding,
    PrecedingSibling,
    Current,
}

impl Axis {
    pub(crate) fn from_name(name: &str) -> Option<Axis> {
        Some(match name {
            "ancestor" => Axis::Ancestor,
            "ancestor-or-self" => Axis::AncestorOrSelf,
            "attribute" => Axis::Attribute,
            "child" => Axis::Child,
            "descendant" => Axis::Descendant,
            "descendant-or-self" => Axis::DescendantOrSelf,
            "following" => Axis::Following,
            "following-sibling" => Axis::FollowingSibling,
            "namespace" => Axis::Namespace,
            "parent" => Axis::Parent,
            "preceding" => Axis::Preceding,
            "preceding-sibling" => Axis::PrecedingSibling,
            "self" => Axis::Current,
            _ => return None,
        })
    }

    /// Reverse axes number their nodes in reverse document order.
    pub(crate) fn is_reverse(self) -> bool {
        matches!(
            self,
            Axis::Ancestor | Axis::AncestorOrSelf | Axis::Preceding | Axis::PrecedingSibling
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum NodeTest {
    /// A QName compared as written (or through the namespace map).
    Name(String),
    /// `prefix:*`
    PrefixWildcard(String),
    /// `*`
    Any,
    Node,
    Text,
    Comment,
    Pi(Option<String>),
}

#[derive(Debug, Clone)]
pub(crate) struct Step {
    pub axis: Axis,
    pub test: NodeTest,
    pub predicates: Vec<Expr>,
}

#[derive(Debug, Clone)]
pub(crate) enum PathStart {
    Root,
    Context,
    Expr(Box<Expr>),
}

#[derive(Debug, Clone)]
pub(crate) struct PathExpr {
    pub start: PathStart,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinOp {
    Eq,
    Neq,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

#[derive(Debug, Clone)]
pub(crate) enum Expr {
    Or(Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Neg(Box<Expr>),
    Union(Box<Expr>, Box<Expr>),
    Number(f64),
    Literal(String),
    Variable(String),
    Function(String, Vec<Expr>),
    Filter(Box<Expr>, Vec<Expr>),
    Path(PathExpr),
}
