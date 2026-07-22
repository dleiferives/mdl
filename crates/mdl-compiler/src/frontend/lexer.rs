//! Bounded lexer for the first source-language grammar.

use std::error::Error;
use std::fmt;

use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::frontend::FrontendLimits;
use crate::frontend::token::{Token, TokenBuffer, TokenKind};
use crate::source::{FileId, Origin, OriginError, SourceContext, SourceError, Span};

const UNKNOWN_CHARACTER_CODE: &str = "frontend.lex.unknown-character";
const INVALID_STRING_ESCAPE_CODE: &str = "frontend.lex.invalid-string-escape";
const UNTERMINATED_STRING_CODE: &str = "frontend.lex.unterminated-string";
const TRUNCATED_CODE: &str = "frontend.lex.truncated";

/// Tokens and recoverable lexical diagnostics for one source file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LexOutput {
    tokens: TokenBuffer,
    diagnostics: Option<Diagnostics>,
    truncated: bool,
}

impl LexOutput {
    #[cfg(test)]
    pub(crate) const fn tokens(&self) -> &TokenBuffer {
        &self.tokens
    }

    #[cfg(test)]
    pub(crate) const fn diagnostics(&self) -> Option<&Diagnostics> {
        self.diagnostics.as_ref()
    }

    #[cfg(test)]
    pub(crate) const fn is_truncated(&self) -> bool {
        self.truncated
    }

    pub(crate) fn into_parts(self) -> (TokenBuffer, Option<Diagnostics>) {
        (self.tokens, self.diagnostics)
    }
}

/// Infrastructure failure that prevents a lexical result from being formed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum LexerError {
    Source(SourceError),
    Origin(OriginError),
}

impl fmt::Display for LexerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(error) => write!(formatter, "cannot lex source: {error}"),
            Self::Origin(error) => write!(formatter, "cannot record lexical diagnostic: {error}"),
        }
    }
}

impl Error for LexerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Source(error) => Some(error),
            Self::Origin(error) => Some(error),
        }
    }
}

impl From<SourceError> for LexerError {
    fn from(error: SourceError) -> Self {
        Self::Source(error)
    }
}

impl From<OriginError> for LexerError {
    fn from(error: OriginError) -> Self {
        Self::Origin(error)
    }
}

/// Lexes one immutable source file without borrowing or copying token text.
pub(crate) fn lex(
    sources: &mut SourceContext,
    file: FileId,
    limits: FrontendLimits,
) -> Result<LexOutput, LexerError> {
    let scan = {
        let source = sources
            .files()
            .get(file)
            .ok_or(SourceError::InvalidFile { file })?;
        scan_source(sources, file, source.text(), limits)?
    };

    let diagnostics = materialize_diagnostics(sources, scan.findings, scan.truncation)?;
    Ok(LexOutput {
        tokens: TokenBuffer::new(scan.tokens),
        diagnostics,
        truncated: scan.truncation.is_some(),
    })
}

#[derive(Debug)]
struct ScanOutput {
    tokens: Vec<Token>,
    findings: Vec<PendingDiagnostic>,
    truncation: Option<Truncation>,
}

#[derive(Debug)]
struct PendingDiagnostic {
    code: &'static str,
    message: String,
    span: Span,
}

#[derive(Clone, Copy, Debug)]
struct Truncation {
    span: Span,
    diagnostics_omitted: bool,
    tokens_omitted: bool,
}

impl Truncation {
    const fn diagnostic(span: Span) -> Self {
        Self {
            span,
            diagnostics_omitted: true,
            tokens_omitted: false,
        }
    }

    const fn token(span: Span) -> Self {
        Self {
            span,
            diagnostics_omitted: false,
            tokens_omitted: true,
        }
    }

    fn note_diagnostic(&mut self) {
        self.diagnostics_omitted = true;
    }

    fn note_token(&mut self) {
        self.tokens_omitted = true;
    }

    const fn message(self) -> &'static str {
        match (self.diagnostics_omitted, self.tokens_omitted) {
            (true, true) => "lexing truncated after reaching diagnostic and token limits",
            (true, false) => "additional lexical diagnostics were omitted",
            (false, true) => "lexing truncated after reaching the token limit",
            (false, false) => "lexing truncated after reaching a resource limit",
        }
    }
}

fn scan_source(
    sources: &SourceContext,
    file: FileId,
    text: &str,
    limits: FrontendLimits,
) -> Result<ScanOutput, SourceError> {
    let bytes = text.as_bytes();
    let file_end = u32::try_from(bytes.len()).map_err(|_| SourceError::FileTooLarge {
        byte_len: bytes.len(),
    })?;
    let non_eof_capacity = limits.max_tokens() - 1;
    let mut tokens = Vec::new();
    let mut findings = Vec::new();
    let mut truncation = None;
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b' ' | b'\t' | b'\r' | b'\n' => {
                index += 1;
            }
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            byte if is_identifier_start(byte) => {
                let start = index;
                index += 1;
                while index < bytes.len() && is_identifier_continue(bytes[index]) {
                    index += 1;
                }
                let kind = identifier_kind(&text[start..index]);
                let span = span(sources, file, start, index, bytes.len())?;
                if !push_token(&mut tokens, kind, span, non_eof_capacity, &mut truncation) {
                    break;
                }
            }
            b'0'..=b'9' => {
                let start = index;
                index += 1;
                while index < bytes.len() && bytes[index].is_ascii_digit() {
                    index += 1;
                }
                let kind = if bytes.get(index) == Some(&b'.')
                    && bytes.get(index + 1).is_some_and(u8::is_ascii_digit)
                {
                    index += 1;
                    while index < bytes.len() && bytes[index].is_ascii_digit() {
                        index += 1;
                    }
                    TokenKind::DecimalNumber
                } else {
                    TokenKind::DecimalInteger
                };
                let span = span(sources, file, start, index, bytes.len())?;
                if !push_token(&mut tokens, kind, span, non_eof_capacity, &mut truncation) {
                    break;
                }
            }
            b'"' => {
                let start = index;
                index = scan_string_literal(
                    sources,
                    file,
                    text,
                    index,
                    &mut findings,
                    &mut truncation,
                    limits.max_diagnostics(),
                )?;
                let string_span = span(sources, file, start, index, bytes.len())?;
                if !push_token(
                    &mut tokens,
                    TokenKind::StringLiteral,
                    string_span,
                    non_eof_capacity,
                    &mut truncation,
                ) {
                    break;
                }
            }
            _ => {
                let (kind, width) = punctuation(bytes, index);
                if let Some(kind) = kind {
                    let start = index;
                    index += width;
                    let span = span(sources, file, start, index, bytes.len())?;
                    if !push_token(&mut tokens, kind, span, non_eof_capacity, &mut truncation) {
                        break;
                    }
                } else {
                    index = scan_unknown_character(
                        sources,
                        file,
                        text,
                        index,
                        &mut findings,
                        &mut truncation,
                        limits.max_diagnostics(),
                    )?;
                }
            }
        }
    }

    finish_scan(sources, file, file_end, tokens, findings, truncation)
}

fn finish_scan(
    sources: &SourceContext,
    file: FileId,
    file_end: u32,
    mut tokens: Vec<Token>,
    findings: Vec<PendingDiagnostic>,
    truncation: Option<Truncation>,
) -> Result<ScanOutput, SourceError> {
    let eof_span = sources.span(file, file_end, file_end)?;
    tokens.push(Token::new(TokenKind::EndOfFile, eof_span));
    Ok(ScanOutput {
        tokens,
        findings,
        truncation,
    })
}

fn scan_unknown_character(
    sources: &SourceContext,
    file: FileId,
    text: &str,
    start: usize,
    findings: &mut Vec<PendingDiagnostic>,
    truncation: &mut Option<Truncation>,
    max_diagnostics: usize,
) -> Result<usize, SourceError> {
    let character = text[start..]
        .chars()
        .next()
        .ok_or(SourceError::NotCharBoundary {
            file,
            offset: u32::try_from(start).unwrap_or(u32::MAX),
        })?;
    let end = start + character.len_utf8();
    let character_span = span(sources, file, start, end, text.len())?;
    record_unknown(
        findings,
        truncation,
        character,
        character_span,
        max_diagnostics,
    );
    Ok(end)
}

fn scan_string_literal(
    sources: &SourceContext,
    file: FileId,
    text: &str,
    start: usize,
    findings: &mut Vec<PendingDiagnostic>,
    truncation: &mut Option<Truncation>,
    max_diagnostics: usize,
) -> Result<usize, SourceError> {
    let bytes = text.as_bytes();
    let mut index = start + 1;
    let mut terminated = false;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                index += 1;
                terminated = true;
                break;
            }
            b'\n' | b'\r' => break,
            b'\\' => {
                let escape_start = index;
                index += 1;
                if index >= bytes.len() || matches!(bytes[index], b'\n' | b'\r') {
                    break;
                }
                let escaped = bytes[index];
                if matches!(escaped, b'"' | b'\\' | b'n' | b'r' | b't' | b'0') {
                    index += 1;
                    continue;
                }
                index = next_scalar_end(text, file, index)?;
                let escape_span = span(sources, file, escape_start, index, bytes.len())?;
                record_finding(
                    findings,
                    truncation,
                    INVALID_STRING_ESCAPE_CODE,
                    "invalid string escape; expected `\\\"`, `\\\\`, `\\n`, `\\r`, `\\t`, or `\\0`",
                    escape_span,
                    max_diagnostics,
                );
            }
            byte if byte.is_ascii() => index += 1,
            _ => index = next_scalar_end(text, file, index)?,
        }
    }

    if !terminated {
        let string_span = span(sources, file, start, index, bytes.len())?;
        record_finding(
            findings,
            truncation,
            UNTERMINATED_STRING_CODE,
            "unterminated string literal",
            string_span,
            max_diagnostics,
        );
    }
    Ok(index)
}

fn next_scalar_end(text: &str, file: FileId, index: usize) -> Result<usize, SourceError> {
    let character = text[index..]
        .chars()
        .next()
        .ok_or(SourceError::NotCharBoundary {
            file,
            offset: u32::try_from(index).unwrap_or(u32::MAX),
        })?;
    Ok(index + character.len_utf8())
}

fn push_token(
    tokens: &mut Vec<Token>,
    kind: TokenKind,
    span: Span,
    non_eof_capacity: usize,
    truncation: &mut Option<Truncation>,
) -> bool {
    if tokens.len() < non_eof_capacity {
        tokens.push(Token::new(kind, span));
        return true;
    }

    match truncation {
        Some(existing) => existing.note_token(),
        None => *truncation = Some(Truncation::token(span)),
    }
    false
}

fn record_unknown(
    findings: &mut Vec<PendingDiagnostic>,
    truncation: &mut Option<Truncation>,
    character: char,
    span: Span,
    max_diagnostics: usize,
) {
    record_finding(
        findings,
        truncation,
        UNKNOWN_CHARACTER_CODE,
        format!("unknown character {character:?}"),
        span,
        max_diagnostics,
    );
}

fn record_finding(
    findings: &mut Vec<PendingDiagnostic>,
    truncation: &mut Option<Truncation>,
    code: &'static str,
    message: impl Into<String>,
    span: Span,
    max_diagnostics: usize,
) {
    if findings.len() < max_diagnostics {
        findings.push(PendingDiagnostic {
            code,
            message: message.into(),
            span,
        });
        return;
    }

    match truncation {
        Some(existing) => existing.note_diagnostic(),
        None => *truncation = Some(Truncation::diagnostic(span)),
    }
}

fn materialize_diagnostics(
    sources: &mut SourceContext,
    findings: Vec<PendingDiagnostic>,
    truncation: Option<Truncation>,
) -> Result<Option<Diagnostics>, OriginError> {
    let additional = usize::from(truncation.is_some());
    let mut diagnostics = Vec::with_capacity(findings.len() + additional);

    for finding in findings {
        let origin = sources.add_origin(Origin::Source(finding.span))?;
        diagnostics.push(Diagnostic::new(finding.code, finding.message, origin));
    }

    if let Some(truncation) = truncation {
        let origin = sources.add_origin(Origin::Source(truncation.span))?;
        diagnostics.push(Diagnostic::new(
            TRUNCATED_CODE,
            truncation.message(),
            origin,
        ));
    }

    Ok(Diagnostics::from_findings(diagnostics))
}

fn punctuation(bytes: &[u8], index: usize) -> (Option<TokenKind>, usize) {
    let byte = bytes[index];
    let next = bytes.get(index + 1).copied();
    let third = bytes.get(index + 2).copied();
    if (byte, next, third) == (b'.', Some(b'.'), Some(b'.')) {
        return (Some(TokenKind::Ellipsis), 3);
    }
    match (byte, next) {
        (b':', Some(b'=')) => (Some(TokenKind::ColonEqual), 2),
        (b'-', Some(b'>')) => (Some(TokenKind::Arrow), 2),
        (b'+', Some(b'%')) => (Some(TokenKind::PlusPercent), 2),
        (b'-', Some(b'%')) => (Some(TokenKind::MinusPercent), 2),
        (b'=', Some(b'=')) => (Some(TokenKind::EqualEqual), 2),
        (b'=', Some(b'>')) => (Some(TokenKind::FatArrow), 2),
        (b'!', Some(b'=')) => (Some(TokenKind::BangEqual), 2),
        (b'<', Some(b'=')) => (Some(TokenKind::LessEqual), 2),
        (b'>', Some(b'=')) => (Some(TokenKind::GreaterEqual), 2),
        (b'(', _) => (Some(TokenKind::LeftParenthesis), 1),
        (b')', _) => (Some(TokenKind::RightParenthesis), 1),
        (b'{', _) => (Some(TokenKind::LeftBrace), 1),
        (b'}', _) => (Some(TokenKind::RightBrace), 1),
        (b'[', _) => (Some(TokenKind::LeftBracket), 1),
        (b']', _) => (Some(TokenKind::RightBracket), 1),
        (b':', _) => (Some(TokenKind::Colon), 1),
        (b';', _) => (Some(TokenKind::Semicolon), 1),
        (b',', _) => (Some(TokenKind::Comma), 1),
        (b'.', _) => (Some(TokenKind::Dot), 1),
        (b'|', _) => (Some(TokenKind::Pipe), 1),
        (b'=', _) => (Some(TokenKind::Equal), 1),
        (b'!', _) => (Some(TokenKind::Bang), 1),
        (b'<', _) => (Some(TokenKind::Less), 1),
        (b'>', _) => (Some(TokenKind::Greater), 1),
        (b'-', _) => (Some(TokenKind::Minus), 1),
        (b'~', _) => (Some(TokenKind::Tilde), 1),
        (b'^', _) => (Some(TokenKind::Caret), 1),
        _ => (None, 1),
    }
}

const fn is_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

const fn is_identifier_continue(byte: u8) -> bool {
    is_identifier_start(byte) || byte.is_ascii_digit()
}

fn identifier_kind(identifier: &str) -> TokenKind {
    match identifier {
        "fn" => TokenKind::KeywordFn,
        "pub" => TokenKind::KeywordPub,
        "export" => TokenKind::KeywordExport,
        "import" => TokenKind::KeywordImport,
        "struct" => TokenKind::KeywordStruct,
        "enum" => TokenKind::KeywordEnum,
        "unsafe" => TokenKind::KeywordUnsafe,
        "minecraft" => TokenKind::KeywordMinecraft,
        "run" => TokenKind::KeywordRun,
        "on" => TokenKind::KeywordOn,
        "const" => TokenKind::KeywordConst,
        "var" => TokenKind::KeywordVar,
        "if" => TokenKind::KeywordIf,
        "else" => TokenKind::KeywordElse,
        "while" => TokenKind::KeywordWhile,
        "switch" => TokenKind::KeywordSwitch,
        "break" => TokenKind::KeywordBreak,
        "continue" => TokenKind::KeywordContinue,
        "return" => TokenKind::KeywordReturn,
        "true" => TokenKind::KeywordTrue,
        "false" => TokenKind::KeywordFalse,
        "Bool" => TokenKind::KeywordBool,
        "Int32" => TokenKind::KeywordInt32,
        "Void" => TokenKind::KeywordVoid,
        _ => TokenKind::Identifier,
    }
}

fn span(
    sources: &SourceContext,
    file: FileId,
    start: usize,
    end: usize,
    file_len: usize,
) -> Result<Span, SourceError> {
    let start =
        u32::try_from(start).map_err(|_| SourceError::FileTooLarge { byte_len: file_len })?;
    let end = u32::try_from(end).map_err(|_| SourceError::FileTooLarge { byte_len: file_len })?;
    sources.span(file, start, end)
}

#[cfg(test)]
mod tests {
    use super::{
        INVALID_STRING_ESCAPE_CODE, LexOutput, LexerError, TRUNCATED_CODE, UNKNOWN_CHARACTER_CODE,
        UNTERMINATED_STRING_CODE, lex,
    };
    use crate::frontend::FrontendLimits;
    use crate::frontend::token::TokenKind;
    use crate::source::{FileId, Origin, SourceContext, SourceError, Span};

    fn limits(max_tokens: usize, max_diagnostics: usize) -> FrontendLimits {
        FrontendLimits::new(max_tokens, 256, max_diagnostics).unwrap()
    }

    fn lex_text(text: &str, limits: FrontendLimits) -> (SourceContext, FileId, LexOutput) {
        let mut sources = SourceContext::new();
        let file = sources.add_file("test.mdl", text).unwrap();
        let output = lex(&mut sources, file, limits).unwrap();
        (sources, file, output)
    }

    fn default_lex(text: &str) -> (SourceContext, FileId, LexOutput) {
        lex_text(text, FrontendLimits::DEFAULT)
    }

    fn token_table(output: &LexOutput) -> Vec<(TokenKind, u32, u32)> {
        output
            .tokens()
            .iter()
            .map(|token| {
                let span = token.span();
                (token.kind(), span.start(), span.end())
            })
            .collect()
    }

    fn diagnostic_spans(sources: &SourceContext, output: &LexOutput) -> Vec<Span> {
        output
            .diagnostics()
            .map_or(&[][..], |diagnostics| diagnostics.findings())
            .iter()
            .map(|diagnostic| {
                sources
                    .resolve_origin_span(diagnostic.origin())
                    .expect("lexical diagnostics have direct source origins")
            })
            .collect()
    }

    fn diagnostic_codes(output: &LexOutput) -> Vec<&'static str> {
        output
            .diagnostics()
            .map_or(&[][..], |diagnostics| diagnostics.findings())
            .iter()
            .map(crate::diagnostic::Diagnostic::code)
            .collect()
    }

    #[test]
    fn recognizes_every_keyword_and_identifier_boundaries() {
        let text = "fn pub export import struct enum const var if else while switch return true false Bool Int32 Void fn_ enum_ switch_ _if bool int32 void x X1 _";
        let (_, _, output) = default_lex(text);
        let expected_kinds = [
            TokenKind::KeywordFn,
            TokenKind::KeywordPub,
            TokenKind::KeywordExport,
            TokenKind::KeywordImport,
            TokenKind::KeywordStruct,
            TokenKind::KeywordEnum,
            TokenKind::KeywordConst,
            TokenKind::KeywordVar,
            TokenKind::KeywordIf,
            TokenKind::KeywordElse,
            TokenKind::KeywordWhile,
            TokenKind::KeywordSwitch,
            TokenKind::KeywordReturn,
            TokenKind::KeywordTrue,
            TokenKind::KeywordFalse,
            TokenKind::KeywordBool,
            TokenKind::KeywordInt32,
            TokenKind::KeywordVoid,
            TokenKind::Identifier,
            TokenKind::Identifier,
            TokenKind::Identifier,
            TokenKind::Identifier,
            TokenKind::Identifier,
            TokenKind::Identifier,
            TokenKind::Identifier,
            TokenKind::Identifier,
            TokenKind::Identifier,
            TokenKind::Identifier,
            TokenKind::EndOfFile,
        ];
        let actual_kinds: Vec<_> = output.tokens().iter().map(|token| token.kind()).collect();

        assert_eq!(actual_kinds, expected_kinds);
        assert!(output.diagnostics().is_none());
        assert!(!output.is_truncated());

        for token in output.tokens().as_slice() {
            let span = token.span();
            if token.kind() != TokenKind::EndOfFile {
                assert!(!span.is_empty());
            }
        }
    }

    #[test]
    fn decimal_tokens_contain_digits_only() {
        let (sources, _, output) = default_lex("0 007 42a 9_2");
        let kinds: Vec<_> = output.tokens().iter().map(|token| token.kind()).collect();
        assert_eq!(
            kinds,
            [
                TokenKind::DecimalInteger,
                TokenKind::DecimalInteger,
                TokenKind::DecimalInteger,
                TokenKind::Identifier,
                TokenKind::DecimalInteger,
                TokenKind::Identifier,
                TokenKind::EndOfFile,
            ]
        );
        let spellings: Vec<_> = output
            .tokens()
            .iter()
            .map(|token| sources.files().slice(token.span()).unwrap())
            .collect();
        assert_eq!(spellings, ["0", "007", "42", "a", "9", "_2", ""]);
    }

    #[test]
    fn recognizes_all_punctuation_with_maximal_munch_and_exact_ranges() {
        let text = "(){}:;,.=->=>...! == != < <= > >=";
        let (_, _, output) = default_lex(text);
        assert_eq!(
            token_table(&output),
            [
                (TokenKind::LeftParenthesis, 0, 1),
                (TokenKind::RightParenthesis, 1, 2),
                (TokenKind::LeftBrace, 2, 3),
                (TokenKind::RightBrace, 3, 4),
                (TokenKind::Colon, 4, 5),
                (TokenKind::Semicolon, 5, 6),
                (TokenKind::Comma, 6, 7),
                (TokenKind::Dot, 7, 8),
                (TokenKind::Equal, 8, 9),
                (TokenKind::Arrow, 9, 11),
                (TokenKind::FatArrow, 11, 13),
                (TokenKind::Ellipsis, 13, 16),
                (TokenKind::Bang, 16, 17),
                (TokenKind::EqualEqual, 18, 20),
                (TokenKind::BangEqual, 21, 23),
                (TokenKind::Less, 24, 25),
                (TokenKind::LessEqual, 26, 28),
                (TokenKind::Greater, 29, 30),
                (TokenKind::GreaterEqual, 31, 33),
                (TokenKind::EndOfFile, 33, 33),
            ]
        );
        assert!(output.diagnostics().is_none());
    }

    #[test]
    fn whitespace_and_line_comments_are_trivia() {
        let text = "\t\r\n fn // ignored: é -> !=\r\nconst// through EOF";
        let (sources, _, output) = default_lex(text);
        let tokens = output.tokens().as_slice();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].kind(), TokenKind::KeywordFn);
        assert_eq!(sources.files().slice(tokens[0].span()).unwrap(), "fn");
        assert_eq!(tokens[1].kind(), TokenKind::KeywordConst);
        assert_eq!(sources.files().slice(tokens[1].span()).unwrap(), "const");
        assert_eq!(tokens[2].kind(), TokenKind::EndOfFile);
        assert!(output.diagnostics().is_none());
        assert_eq!(sources.origin_count(), 1);
    }

    #[test]
    fn eof_is_empty_at_the_exact_file_end() {
        for text in ["", " ", "fn", "// comment", "é"] {
            let (_, file, output) = default_lex(text);
            let eof = output.tokens().as_slice().last().unwrap();
            assert_eq!(eof.kind(), TokenKind::EndOfFile);
            assert_eq!(eof.span().file(), file);
            assert_eq!(eof.span().start(), u32::try_from(text.len()).unwrap());
            assert_eq!(eof.span().end(), u32::try_from(text.len()).unwrap());
            assert!(eof.span().is_empty());
        }
    }

    #[test]
    fn each_non_ascii_scalar_gets_one_exact_diagnostic() {
        let (sources, file, output) = default_lex("aé中🦀z");
        assert_eq!(
            token_table(&output),
            [
                (TokenKind::Identifier, 0, 1),
                (TokenKind::Identifier, 10, 11),
                (TokenKind::EndOfFile, 11, 11),
            ]
        );
        assert_eq!(
            diagnostic_spans(&sources, &output),
            [
                sources.span(file, 1, 3).unwrap(),
                sources.span(file, 3, 6).unwrap(),
                sources.span(file, 6, 10).unwrap(),
            ]
        );
        assert_eq!(
            diagnostic_codes(&output),
            [
                UNKNOWN_CHARACTER_CODE,
                UNKNOWN_CHARACTER_CODE,
                UNKNOWN_CHARACTER_CODE,
            ]
        );
        let messages: Vec<_> = output
            .diagnostics()
            .unwrap()
            .findings()
            .iter()
            .map(crate::diagnostic::Diagnostic::message)
            .collect();
        assert_eq!(
            messages,
            [
                "unknown character 'é'",
                "unknown character '中'",
                "unknown character '🦀'",
            ]
        );
    }

    #[test]
    fn unknown_ascii_and_malformed_operators_recover_at_one_scalar_each() {
        let text = "- / + & | ? => && ||";
        let (sources, file, output) = default_lex(text);
        assert_eq!(
            token_table(&output),
            [
                (TokenKind::Minus, 0, 1),
                (TokenKind::Pipe, 8, 9),
                (TokenKind::FatArrow, 12, 14),
                (TokenKind::Pipe, 18, 19),
                (TokenKind::Pipe, 19, 20),
                (TokenKind::EndOfFile, 20, 20),
            ]
        );
        assert_eq!(
            diagnostic_spans(&sources, &output),
            [2..3, 4..5, 6..7, 10..11, 15..16, 16..17]
                .map(|range| sources.span(file, range.start, range.end).unwrap())
        );
        assert!(
            diagnostic_codes(&output)
                .iter()
                .all(|code| *code == UNKNOWN_CHARACTER_CODE)
        );
    }

    #[test]
    fn string_literals_preserve_utf8_and_validate_the_closed_escape_set() {
        let text =
            r#""cells" "héllo" "quote: \" slash: \\ line: \n tab: \t zero: \0" "bad: \q" "open"#;
        let (sources, file, output) = default_lex(text);
        let strings = output
            .tokens()
            .iter()
            .filter(|token| token.kind() == TokenKind::StringLiteral)
            .map(|token| sources.files().slice(token.span()).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(strings.len(), 5);
        assert_eq!(strings[0], "\"cells\"");
        assert_eq!(strings[1], "\"héllo\"");
        assert_eq!(
            diagnostic_codes(&output),
            [INVALID_STRING_ESCAPE_CODE, UNTERMINATED_STRING_CODE]
        );
        let spans = diagnostic_spans(&sources, &output);
        assert_eq!(sources.files().slice(spans[0]).unwrap(), "\\q");
        assert_eq!(spans[1].end(), u32::try_from(text.len()).unwrap());
        assert_eq!(spans[1].file(), file);
    }

    #[test]
    fn diagnostic_limit_adds_one_final_marker_only_after_overflow() {
        let (_, _, exact) = lex_text("@@", limits(10, 2));
        assert_eq!(
            diagnostic_codes(&exact),
            [UNKNOWN_CHARACTER_CODE, UNKNOWN_CHARACTER_CODE]
        );
        assert!(!exact.is_truncated());

        let (sources, file, overflow) = lex_text("@#$%", limits(10, 2));
        assert_eq!(
            diagnostic_codes(&overflow),
            [
                UNKNOWN_CHARACTER_CODE,
                UNKNOWN_CHARACTER_CODE,
                TRUNCATED_CODE
            ]
        );
        assert_eq!(
            diagnostic_spans(&sources, &overflow)[2],
            sources.span(file, 2, 3).unwrap()
        );
        assert!(overflow.is_truncated());

        let (_, _, zero) = lex_text("@#", limits(10, 0));
        assert_eq!(diagnostic_codes(&zero), [TRUNCATED_CODE]);
        assert_eq!(zero.diagnostics().unwrap().len(), 1);
    }

    #[test]
    fn token_limit_counts_eof_and_marks_the_first_omitted_token() {
        let (_, _, exact) = lex_text("fn var", limits(3, 10));
        assert_eq!(
            exact
                .tokens()
                .iter()
                .map(|token| token.kind())
                .collect::<Vec<_>>(),
            [
                TokenKind::KeywordFn,
                TokenKind::KeywordVar,
                TokenKind::EndOfFile,
            ]
        );
        assert!(!exact.is_truncated());

        let (sources, file, overflow) = lex_text("fn var if", limits(3, 10));
        assert_eq!(overflow.tokens().len(), 3);
        assert_eq!(overflow.tokens().as_slice()[2].kind(), TokenKind::EndOfFile);
        assert_eq!(diagnostic_codes(&overflow), [TRUNCATED_CODE]);
        assert_eq!(
            diagnostic_spans(&sources, &overflow),
            [sources.span(file, 7, 9).unwrap()]
        );
        assert!(overflow.is_truncated());

        let (_, _, eof_only) = lex_text("fn", limits(1, 10));
        assert_eq!(eof_only.tokens().len(), 1);
        assert_eq!(eof_only.tokens().as_slice()[0].kind(), TokenKind::EndOfFile);
        assert_eq!(diagnostic_codes(&eof_only), [TRUNCATED_CODE]);
    }

    #[test]
    fn huge_token_input_is_bounded_and_eof_remains_at_file_end() {
        let text = "x ".repeat(200_000);
        let (_, _, output) = lex_text(&text, limits(8, 3));
        assert_eq!(output.tokens().len(), 8);
        assert_eq!(
            output.tokens().as_slice().last().unwrap().span().start(),
            u32::try_from(text.len()).unwrap()
        );
        assert_eq!(diagnostic_codes(&output), [TRUNCATED_CODE]);
    }

    #[test]
    fn a_huge_decimal_is_one_token_without_numeric_interpretation() {
        let text = "9".repeat(100_000);
        let (_, _, output) = default_lex(&text);
        assert_eq!(
            token_table(&output),
            [
                (TokenKind::DecimalInteger, 0, 100_000),
                (TokenKind::EndOfFile, 100_000, 100_000),
            ]
        );
        assert!(output.diagnostics().is_none());
    }

    #[test]
    fn combined_limits_still_emit_exactly_one_final_marker() {
        let (_, _, output) = lex_text("@ # fn var", limits(2, 1));
        assert_eq!(output.tokens().len(), 2);
        assert_eq!(
            diagnostic_codes(&output),
            [UNKNOWN_CHARACTER_CODE, TRUNCATED_CODE]
        );
        let diagnostics = output.diagnostics().unwrap();
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(
            diagnostics.findings().last().unwrap().message(),
            "lexing truncated after reaching diagnostic and token limits"
        );
    }

    #[test]
    fn clean_lexing_allocates_no_origins() {
        let (sources, _, output) = default_lex("fn main() -> Void { return; }");
        assert!(output.diagnostics().is_none());
        assert_eq!(sources.origin_count(), 1);
    }

    #[test]
    fn lexical_diagnostics_have_direct_source_origins() {
        let (sources, file, output) = default_lex("@");
        let diagnostic = &output.diagnostics().unwrap().findings()[0];
        assert_eq!(
            sources.origin(diagnostic.origin()),
            Some(&Origin::Source(sources.span(file, 0, 1).unwrap()))
        );
    }

    #[test]
    fn invalid_file_is_reported_as_an_infrastructure_error() {
        let mut donor = SourceContext::new();
        let absent_file = donor.add_file("donor.mdl", "").unwrap();
        let mut sources = SourceContext::new();

        let result = lex(&mut sources, absent_file, FrontendLimits::DEFAULT);
        assert_eq!(
            result,
            Err(LexerError::Source(SourceError::InvalidFile {
                file: absent_file,
            }))
        );
    }

    #[test]
    fn repeated_lexing_is_deterministic() {
        let text = "fn f(x: Int32) -> Bool { return x != 0; } @é";
        let (first_sources, _, first) = default_lex(text);
        let (second_sources, _, second) = default_lex(text);

        assert_eq!(first.tokens(), second.tokens());
        assert_eq!(diagnostic_codes(&first), diagnostic_codes(&second));
        assert_eq!(
            diagnostic_spans(&first_sources, &first),
            diagnostic_spans(&second_sources, &second)
        );
        let first_messages: Vec<_> = first
            .diagnostics()
            .unwrap()
            .findings()
            .iter()
            .map(crate::diagnostic::Diagnostic::message)
            .collect();
        let second_messages: Vec<_> = second
            .diagnostics()
            .unwrap()
            .findings()
            .iter()
            .map(crate::diagnostic::Diagnostic::message)
            .collect();
        assert_eq!(first_messages, second_messages);
    }
}
