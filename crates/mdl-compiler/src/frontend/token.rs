//! Closed lexical token vocabulary.

use crate::source::Span;

/// One kind in the first source-language token vocabulary.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum TokenKind {
    Identifier,
    DecimalInteger,
    DecimalNumber,
    StringLiteral,
    KeywordFn,
    KeywordPub,
    KeywordExport,
    KeywordImport,
    KeywordStruct,
    KeywordEnum,
    KeywordUnsafe,
    KeywordMinecraft,
    KeywordRun,
    KeywordOn,
    KeywordConst,
    KeywordVar,
    KeywordIf,
    KeywordElse,
    KeywordWhile,
    KeywordSwitch,
    KeywordBreak,
    KeywordContinue,
    KeywordReturn,
    KeywordTrue,
    KeywordFalse,
    KeywordBool,
    KeywordInt32,
    KeywordVoid,
    LeftParenthesis,
    RightParenthesis,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    Colon,
    ColonEqual,
    Semicolon,
    Comma,
    Dot,
    Ellipsis,
    Pipe,
    Equal,
    Arrow,
    FatArrow,
    Minus,
    PlusPercent,
    MinusPercent,
    Tilde,
    Caret,
    Bang,
    EqualEqual,
    BangEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    EndOfFile,
}

/// One token and its exact half-open UTF-8 byte range.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct Token {
    kind: TokenKind,
    span: Span,
}

impl Token {
    pub(super) const fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }

    pub(crate) const fn kind(self) -> TokenKind {
        self.kind
    }

    pub(crate) const fn span(self) -> Span {
        self.span
    }
}

/// Dense, source-ordered tokens for one file, including exactly one EOF token.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TokenBuffer {
    tokens: Box<[Token]>,
}

impl TokenBuffer {
    pub(super) fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens: tokens.into_boxed_slice(),
        }
    }

    pub(crate) fn as_slice(&self) -> &[Token] {
        &self.tokens
    }

    #[cfg(test)]
    pub(crate) fn iter(&self) -> impl ExactSizeIterator<Item = &Token> {
        self.tokens.iter()
    }

    pub(crate) const fn len(&self) -> usize {
        self.tokens.len()
    }
}

impl<'a> IntoIterator for &'a TokenBuffer {
    type Item = &'a Token;
    type IntoIter = std::slice::Iter<'a, Token>;

    fn into_iter(self) -> Self::IntoIter {
        self.tokens.iter()
    }
}
