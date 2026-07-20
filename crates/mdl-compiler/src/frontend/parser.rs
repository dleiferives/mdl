//! Hand-written recoverable parser for the first scalar source grammar.

use std::error::Error;
use std::fmt;

use super::FrontendLimits;
use super::ast::{
    AstAnonymousStructType, AstAnonymousStructTypeKind, AstAssignment, AstBindingKind, AstBlock,
    AstCall, AstCallStatement, AstComparisonOp, AstCoordinateSigil, AstDeclaration,
    AstDestructureTarget, AstDestructureTargetKind, AstDestructuringStatement, AstEnum,
    AstEnumVariant, AstExpression, AstExpressionKind, AstFunction, AstFunctionVisibility, AstIfArm,
    AstIfStatement, AstImport, AstInferredStructEntries, AstInferredStructLiteral, AstModule,
    AstName, AstParameter, AstResultType, AstResultTypeKind, AstReturnStatement, AstRunModifier,
    AstRunStatement, AstSignedInteger, AstStatement, AstStruct, AstStructField,
    AstStructFieldInitializer, AstStructLiteral, AstSwitchExpression, AstSwitchExpressionArm,
    AstSwitchLabel, AstSwitchPattern, AstSwitchPatternKind, AstSwitchStatement,
    AstSwitchStatementArm, AstUnsafeMinecraftStatement, AstValueType, AstValueTypeKind,
    AstWhileStatement, AstWrappingArithmeticOp,
};
use super::token::{Token, TokenBuffer, TokenKind};
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::source::{FileId, Origin, OriginError, SourceContext, SourceError, Span};

const EXPECTED_ITEM: &str = "frontend.parse.expected-item";
const EXPECTED_IDENTIFIER: &str = "frontend.parse.expected-identifier";
const EXPECTED_TOKEN: &str = "frontend.parse.expected-token";
const EXPECTED_TYPE: &str = "frontend.parse.expected-type";
const EXPECTED_STATEMENT: &str = "frontend.parse.expected-statement";
const EXPECTED_EXPRESSION: &str = "frontend.parse.expected-expression";
const EXPECTED_ASSIGNMENT_OR_CALL: &str = "frontend.parse.expected-assignment-or-call";
const EXPECTED_CONST_INITIALIZER: &str = "frontend.parse.expected-const-initializer";
const CHAINED_COMPARISON: &str = "frontend.parse.chained-comparison";
const TRUNCATED: &str = "frontend.parse.truncated";

/// Partial syntax and recoverable parse diagnostics for one clean token buffer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ParseOutput {
    module: AstModule,
    diagnostics: Option<Diagnostics>,
    truncated: bool,
}

impl ParseOutput {
    #[cfg(test)]
    pub(crate) const fn module(&self) -> &AstModule {
        &self.module
    }

    #[cfg(test)]
    pub(crate) const fn diagnostics(&self) -> Option<&Diagnostics> {
        self.diagnostics.as_ref()
    }

    #[cfg(test)]
    pub(crate) const fn is_truncated(&self) -> bool {
        self.truncated
    }

    pub(crate) fn into_parts(self) -> (AstModule, Option<Diagnostics>) {
        (self.module, self.diagnostics)
    }
}

/// Infrastructure failure that prevents a parse result from being formed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ParserError {
    Source(SourceError),
    Origin(OriginError),
}

impl fmt::Display for ParserError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(error) => write!(formatter, "cannot parse source: {error}"),
            Self::Origin(error) => write!(formatter, "cannot record parser diagnostic: {error}"),
        }
    }
}

impl Error for ParserError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Source(error) => Some(error),
            Self::Origin(error) => Some(error),
        }
    }
}

impl From<SourceError> for ParserError {
    fn from(error: SourceError) -> Self {
        Self::Source(error)
    }
}

impl From<OriginError> for ParserError {
    fn from(error: OriginError) -> Self {
        Self::Origin(error)
    }
}

/// Parses one complete lexer-produced token buffer.
pub(crate) fn parse(
    sources: &mut SourceContext,
    file: FileId,
    tokens: &TokenBuffer,
    limits: FrontendLimits,
) -> Result<ParseOutput, ParserError> {
    let parsed = {
        let mut parser = Parser::new(sources, file, tokens.as_slice(), limits);
        let module = parser.parse_module()?;
        ParsedModule {
            module,
            findings: parser.findings,
            truncation: parser.truncation,
        }
    };
    let truncated = parsed.truncation.is_some();
    let diagnostics = materialize_diagnostics(sources, parsed.findings, parsed.truncation)?;
    Ok(ParseOutput {
        module: parsed.module,
        diagnostics,
        truncated,
    })
}

struct ParsedModule {
    module: AstModule,
    findings: Vec<PendingDiagnostic>,
    truncation: Option<PendingDiagnostic>,
}

#[derive(Clone, Debug)]
struct PendingDiagnostic {
    code: &'static str,
    message: String,
    span: Span,
}

struct Parser<'a> {
    sources: &'a SourceContext,
    file: FileId,
    tokens: &'a [Token],
    index: usize,
    limits: FrontendLimits,
    depth: usize,
    item_boundary: bool,
    findings: Vec<PendingDiagnostic>,
    truncation: Option<PendingDiagnostic>,
}

impl<'a> Parser<'a> {
    fn new(
        sources: &'a SourceContext,
        file: FileId,
        tokens: &'a [Token],
        limits: FrontendLimits,
    ) -> Self {
        Self {
            sources,
            file,
            tokens,
            index: 0,
            limits,
            depth: 0,
            item_boundary: false,
            findings: vec![],
            truncation: None,
        }
    }

    fn parse_module(&mut self) -> Result<AstModule, SourceError> {
        let start = self.sources.span(self.file, 0, 0)?;
        let mut imports = vec![];
        let mut structs = vec![];
        let mut enums = vec![];
        let mut functions = vec![];
        while !self.at(TokenKind::EndOfFile) {
            let before = self.index;
            match self.current().kind() {
                TokenKind::KeywordConst => {
                    if self.nth_kind(3) == TokenKind::KeywordImport {
                        if let Some(import) = self.parse_import()? {
                            imports.push(import);
                        }
                    } else if self.nth_kind(3) == TokenKind::KeywordEnum {
                        if let Some(enum_) = self.parse_enum()? {
                            enums.push(enum_);
                        }
                    } else if let Some(struct_) = self.parse_struct()? {
                        structs.push(struct_);
                    }
                }
                TokenKind::KeywordFn | TokenKind::KeywordPub | TokenKind::KeywordExport => {
                    if let Some(function) = self.parse_function()? {
                        functions.push(function);
                    }
                }
                _ => {
                    self.error(
                        EXPECTED_ITEM,
                        "expected an import or function definition",
                        self.current().span(),
                    );
                    self.recover_item();
                }
            }
            self.ensure_progress(before);
        }
        let end = self.current().span();
        Ok(AstModule {
            imports,
            structs,
            enums,
            functions,
            span: self.cover(start, end)?,
        })
    }

    fn parse_enum(&mut self) -> Result<Option<AstEnum>, SourceError> {
        self.item_boundary = false;
        let start = self.bump().span();
        let Some(name_token) = self.expect_identifier("expected an enum type name") else {
            self.recover_item();
            return Ok(None);
        };
        let name = AstName {
            span: name_token.span(),
        };
        let mut clean = self
            .expect(TokenKind::Equal, "expected `=` after the enum type name")
            .is_some();
        clean &= self
            .expect(TokenKind::KeywordEnum, "expected `enum` after `=`")
            .is_some();
        clean &= self
            .expect(TokenKind::LeftBrace, "expected `{` to begin enum variants")
            .is_some();
        let mut variants = vec![];
        while !self.at(TokenKind::RightBrace) && !self.at(TokenKind::EndOfFile) {
            let Some(variant) = self.expect_identifier("expected an enum variant name") else {
                clean = false;
                self.recover_list(TokenKind::RightBrace);
                break;
            };
            variants.push(AstEnumVariant {
                name: AstName {
                    span: variant.span(),
                },
                span: variant.span(),
            });
            if self.eat(TokenKind::Comma).is_some() {
                continue;
            }
            if !self.at(TokenKind::RightBrace) {
                self.error(
                    EXPECTED_TOKEN,
                    "expected `,` or `}` after the enum variant",
                    self.current().span(),
                );
                clean = false;
                self.recover_list(TokenKind::RightBrace);
            }
        }
        let close = self.expect(TokenKind::RightBrace, "expected `}` after enum variants");
        clean &= close.is_some();
        let semicolon = self.expect(TokenKind::Semicolon, "expected `;` after the enum");
        clean &= semicolon.is_some();
        let end = semicolon
            .or(close)
            .map_or_else(|| self.previous_or_current_span(), Token::span);
        let span = self.cover(start, end)?;
        if !clean {
            self.recover_item();
            return Ok(None);
        }
        Ok(Some(AstEnum {
            name,
            variants,
            span,
        }))
    }

    fn parse_struct(&mut self) -> Result<Option<AstStruct>, SourceError> {
        self.item_boundary = false;
        let start = self.bump().span();
        let Some(name_token) = self.expect_identifier("expected a struct type name") else {
            self.recover_item();
            return Ok(None);
        };
        let name = AstName {
            span: name_token.span(),
        };
        let mut clean = self
            .expect(TokenKind::Equal, "expected `=` after the struct type name")
            .is_some();
        clean &= self
            .expect(TokenKind::KeywordStruct, "expected `struct` after `=`")
            .is_some();
        clean &= self
            .expect(TokenKind::LeftBrace, "expected `{` to begin struct fields")
            .is_some();
        let mut fields = vec![];
        while !self.at(TokenKind::RightBrace) && !self.at(TokenKind::EndOfFile) {
            let field_start = self.current().span();
            let Some(field_name) = self.expect_identifier("expected a field name") else {
                clean = false;
                self.recover_list(TokenKind::RightBrace);
                break;
            };
            clean &= self
                .expect(TokenKind::Colon, "expected `:` after the field name")
                .is_some();
            let ty = self.parse_value_type();
            clean &= ty.is_some();
            let end = ty.as_ref().map_or(field_start, |ty| ty.span);
            if let Some(ty) = ty {
                fields.push(AstStructField {
                    name: AstName {
                        span: field_name.span(),
                    },
                    ty,
                    span: self.cover(field_start, end)?,
                });
            }
            if self.eat(TokenKind::Comma).is_some() {
                continue;
            }
            if !self.at(TokenKind::RightBrace) {
                self.error(
                    EXPECTED_TOKEN,
                    "expected `,` or `}` after the field",
                    self.current().span(),
                );
                clean = false;
                self.recover_list(TokenKind::RightBrace);
            }
        }
        let close = self.expect(TokenKind::RightBrace, "expected `}` after struct fields");
        clean &= close.is_some();
        let semicolon = self.expect(TokenKind::Semicolon, "expected `;` after the struct");
        clean &= semicolon.is_some();
        let end = semicolon
            .or(close)
            .map_or_else(|| self.previous_or_current_span(), Token::span);
        let span = self.cover(start, end)?;
        if !clean {
            self.recover_item();
            return Ok(None);
        }
        Ok(Some(AstStruct { name, fields, span }))
    }

    fn parse_import(&mut self) -> Result<Option<AstImport>, SourceError> {
        self.item_boundary = false;
        let start = self.bump().span();
        let Some(binding_token) = self.expect_identifier("expected an import binding name") else {
            self.recover_item();
            return Ok(None);
        };
        let binding = AstName {
            span: binding_token.span(),
        };
        let mut clean = self
            .expect(TokenKind::Equal, "expected `=` after the import binding")
            .is_some();
        clean &= self
            .expect(TokenKind::KeywordImport, "expected `import` after `=`")
            .is_some();
        clean &= self
            .expect(TokenKind::LeftParenthesis, "expected `(` after `import`")
            .is_some();
        let dependency = if self.at(TokenKind::StringLiteral) {
            Some(self.bump().span())
        } else {
            self.error(
                EXPECTED_TOKEN,
                "expected a dependency-name string literal",
                self.current().span(),
            );
            clean = false;
            None
        };
        clean &= self
            .expect(
                TokenKind::RightParenthesis,
                "expected `)` after the dependency name",
            )
            .is_some();
        let semicolon = self.expect(TokenKind::Semicolon, "expected `;` after the import");
        clean &= semicolon.is_some();
        let end = semicolon.map_or_else(|| self.previous_or_current_span(), Token::span);
        let span = self.cover(start, end)?;
        if !clean {
            self.recover_item();
            return Ok(None);
        }
        Ok(dependency.map(|dependency| AstImport {
            binding,
            dependency,
            span,
        }))
    }

    fn parse_function(&mut self) -> Result<Option<AstFunction>, SourceError> {
        self.item_boundary = false;
        let (visibility, visibility_span) = match self.current().kind() {
            TokenKind::KeywordPub => (AstFunctionVisibility::Public, Some(self.bump().span())),
            TokenKind::KeywordExport => (AstFunctionVisibility::Export, Some(self.bump().span())),
            _ => (AstFunctionVisibility::Private, None),
        };
        let start = visibility_span.unwrap_or_else(|| self.current().span());
        if self
            .expect(TokenKind::KeywordFn, "expected `fn` after visibility")
            .is_none()
        {
            self.recover_item_body();
            return Ok(None);
        }
        let Some(name_token) = self.expect_identifier("expected a function name") else {
            self.recover_item_body();
            return Ok(None);
        };
        let name = AstName {
            span: name_token.span(),
        };
        if self
            .expect(
                TokenKind::LeftParenthesis,
                "expected `(` after the function name",
            )
            .is_none()
        {
            self.recover_item_body();
            return Ok(None);
        }
        let mut clean = true;
        let (parameters, parameters_clean) = self.parse_parameters()?;
        clean &= parameters_clean;
        clean &= self
            .expect(
                TokenKind::RightParenthesis,
                "expected `)` after function parameters",
            )
            .is_some();

        let result = if let Some(arrow) = self.eat(TokenKind::Arrow) {
            if let Some(mut result) = self.parse_result_type() {
                result.span = self.cover(arrow.span(), result.span)?;
                Some(result)
            } else {
                clean = false;
                None
            }
        } else {
            None
        };
        let Some(body) = self.parse_block()? else {
            self.item_boundary = false;
            self.recover_item();
            return Ok(None);
        };
        let span = self.cover(start, body.span)?;
        Ok(clean.then_some(AstFunction {
            visibility,
            visibility_span,
            name,
            parameters,
            result,
            body,
            span,
        }))
    }

    fn parse_parameters(&mut self) -> Result<(Vec<AstParameter>, bool), SourceError> {
        let mut parameters = vec![];
        let mut clean = true;
        if self.at(TokenKind::RightParenthesis) || self.at(TokenKind::EndOfFile) {
            return Ok((parameters, clean));
        }

        loop {
            let before = self.index;
            let Some(name_token) = self.expect_identifier("expected a parameter name") else {
                clean = false;
                self.recover_list(TokenKind::RightParenthesis);
                if self.eat(TokenKind::Comma).is_some() {
                    continue;
                }
                break;
            };
            let name = AstName {
                span: name_token.span(),
            };
            clean &= self
                .expect(TokenKind::Colon, "expected `:` after the parameter name")
                .is_some();
            if let Some(ty) = self.parse_value_type() {
                parameters.push(AstParameter {
                    name,
                    span: self.cover(name.span, ty.span)?,
                    ty,
                });
            } else {
                clean = false;
            }

            if self.eat(TokenKind::Comma).is_some() {
                if self.at(TokenKind::RightParenthesis) {
                    break;
                }
            } else if self.at(TokenKind::RightParenthesis) || self.at(TokenKind::EndOfFile) {
                break;
            } else {
                clean = false;
                self.error(
                    EXPECTED_TOKEN,
                    "expected `,` or `)` after the parameter",
                    self.current().span(),
                );
                self.recover_list(TokenKind::RightParenthesis);
                if self.eat(TokenKind::Comma).is_none() {
                    break;
                }
            }
            self.ensure_progress(before);
        }
        Ok((parameters, clean))
    }

    fn parse_value_type(&mut self) -> Option<AstValueType> {
        let token = self.current();
        let kind = match token.kind() {
            TokenKind::LeftBrace => return self.parse_anonymous_struct_type(),
            TokenKind::KeywordBool => AstValueTypeKind::Bool,
            TokenKind::KeywordInt32 => AstValueTypeKind::Int32,
            TokenKind::Identifier
                if self.sources.files().slice(token.span()).ok() == Some("List")
                    && self.nth_kind(1) == TokenKind::Less =>
            {
                self.bump();
                self.bump();
                let element = self.expect(
                    TokenKind::KeywordInt32,
                    "expected `Int32` list element type",
                );
                let close = self.expect(TokenKind::Greater, "expected `>` after list element type");
                let end = close.or(element).map_or(token.span(), Token::span);
                return Some(AstValueType {
                    kind: AstValueTypeKind::ListI32,
                    span: self.cover(token.span(), end).ok()?,
                });
            }
            TokenKind::Identifier
                if self.sources.files().slice(token.span()).ok() == Some("String") =>
            {
                AstValueTypeKind::String
            }
            TokenKind::Identifier => AstValueTypeKind::Named(AstName { span: token.span() }),
            _ => {
                self.error(EXPECTED_TYPE, "expected a value type", token.span());
                return None;
            }
        };
        self.bump();
        Some(AstValueType {
            kind,
            span: token.span(),
        })
    }

    fn parse_anonymous_struct_type(&mut self) -> Option<AstValueType> {
        let open = self.bump();
        if !self.enter_depth(open.span()) {
            return None;
        }
        let named = self.at(TokenKind::Identifier) && self.nth_kind(1) == TokenKind::Colon;
        let mut fields = vec![];
        let mut components = vec![];
        let mut clean = true;
        while !self.at(TokenKind::RightBrace) && !self.at(TokenKind::EndOfFile) {
            if named {
                let start = self.current().span();
                let name = self.expect_identifier("expected an anonymous struct field name");
                clean &= name.is_some();
                clean &= self
                    .expect(TokenKind::Colon, "expected `:` after the field name")
                    .is_some();
                let ty = self.parse_value_type();
                clean &= ty.is_some();
                if let (Some(name), Some(ty)) = (name, ty) {
                    let span = self.cover(start, ty.span).ok()?;
                    fields.push(AstStructField {
                        name: AstName { span: name.span() },
                        ty,
                        span,
                    });
                }
            } else if let Some(ty) = self.parse_value_type() {
                components.push(ty);
            } else {
                clean = false;
            }
            if self.eat(TokenKind::Comma).is_some() {
                continue;
            }
            if !self.at(TokenKind::RightBrace) {
                self.error(
                    EXPECTED_TOKEN,
                    "expected `,` or `}` after the anonymous struct component",
                    self.current().span(),
                );
                clean = false;
                self.recover_list(TokenKind::RightBrace);
            }
        }
        let close = self.expect(TokenKind::RightBrace, "expected `}` after anonymous struct type");
        self.leave_depth();
        clean &= close.is_some();
        let end = close.map_or_else(|| self.previous_or_current_span(), Token::span);
        if (named && fields.is_empty()) || (!named && components.is_empty()) {
            self.error(EXPECTED_TYPE, "anonymous struct types require a component", open.span());
            clean = false;
        }
        clean.then(|| {
            let span = self.cover(open.span(), end).expect("ordered type delimiters");
            AstValueType {
                kind: AstValueTypeKind::Anonymous(AstAnonymousStructType {
                    kind: if named {
                        AstAnonymousStructTypeKind::Named(fields)
                    } else {
                        AstAnonymousStructTypeKind::Positional(components)
                    },
                    span,
                }),
                span,
            }
        })
    }

    fn parse_result_type(&mut self) -> Option<AstResultType> {
        let token = self.current();
        if token.kind() == TokenKind::KeywordVoid {
            self.bump();
            return Some(AstResultType {
                kind: AstResultTypeKind::Void,
                span: token.span(),
            });
        }
        self.parse_value_type().map(|ty| AstResultType {
            kind: AstResultTypeKind::Value(ty.kind),
            span: ty.span,
        })
    }

    fn parse_block(&mut self) -> Result<Option<AstBlock>, SourceError> {
        let Some(open) = self.expect(TokenKind::LeftBrace, "expected `{` to begin a block") else {
            if self.at_function_item_start() {
                self.item_boundary = true;
            }
            return Ok(None);
        };
        if !self.enter_depth(open.span()) {
            self.recover_block_body();
            return Ok(None);
        }
        let mut statements = vec![];
        while !self.at(TokenKind::RightBrace) && !self.at(TokenKind::EndOfFile) {
            if self.at_function_item_start() {
                self.error(
                    EXPECTED_TOKEN,
                    "expected `}` before the next function definition",
                    self.current().span(),
                );
                self.item_boundary = true;
                break;
            }
            let before = self.index;
            let statement = self.parse_statement()?;
            if self.item_boundary {
                break;
            }
            statements.push(statement);
            self.ensure_progress(before);
        }
        if self.item_boundary {
            self.leave_depth();
            return Ok(None);
        }
        let close = self.expect(TokenKind::RightBrace, "expected `}` to close the block");
        self.leave_depth();
        let Some(close) = close else {
            return Ok(None);
        };
        Ok(Some(AstBlock {
            statements,
            closing_brace: close.span(),
            span: self.cover(open.span(), close.span())?,
        }))
    }

    fn parse_statement(&mut self) -> Result<AstStatement, SourceError> {
        match self.current().kind() {
            TokenKind::KeywordConst | TokenKind::KeywordVar => self.parse_declaration(),
            TokenKind::Pipe => self.parse_destructuring_statement(),
            TokenKind::KeywordIf => self.parse_if_statement(),
            TokenKind::KeywordWhile => self.parse_while_statement(),
            TokenKind::KeywordSwitch => self.parse_switch_statement(),
            TokenKind::KeywordBreak => self.parse_loop_control_statement(true),
            TokenKind::KeywordContinue => self.parse_loop_control_statement(false),
            TokenKind::KeywordReturn => self.parse_return_statement(),
            TokenKind::KeywordRun if self.nth_kind(1) == TokenKind::Equal => {
                self.parse_assignment()
            }
            TokenKind::KeywordRun if self.nth_kind(1) == TokenKind::LeftParenthesis => {
                self.parse_call_statement()
            }
            TokenKind::KeywordRun => self.parse_run_statement(),
            TokenKind::KeywordUnsafe => self.parse_unsafe_minecraft_statement(),
            kind if is_identifier_like(kind) && self.nth_kind(1) == TokenKind::Equal => {
                self.parse_assignment()
            }
            kind if is_identifier_like(kind) => self.parse_call_statement(),
            _ => {
                let span = self.current().span();
                self.error(EXPECTED_STATEMENT, "expected a statement", span);
                self.recover_statement();
                Ok(AstStatement::Error(span))
            }
        }
    }

    fn parse_switch_statement(&mut self) -> Result<AstStatement, SourceError> {
        let start = self.bump().span();
        let mut clean = self
            .expect(TokenKind::LeftParenthesis, "expected `(` after `switch`")
            .is_some();
        let scrutinee = self.parse_expression()?;
        clean &= !expression_has_error(&scrutinee);
        clean &= self
            .expect(
                TokenKind::RightParenthesis,
                "expected `)` after switch scrutinee",
            )
            .is_some();
        let Some(open) = self.expect(TokenKind::LeftBrace, "expected `{` to begin switch arms")
        else {
            return self.error_statement(start);
        };
        if !self.enter_depth(open.span()) {
            return self.error_statement(start);
        }
        let mut arms = vec![];
        while !self.at(TokenKind::RightBrace) && !self.at(TokenKind::EndOfFile) {
            let arm_start = self.current().span();
            let label = self.parse_switch_label()?;
            clean &= label.is_some();
            clean &= self
                .expect(TokenKind::FatArrow, "expected `=>` after switch patterns")
                .is_some();
            let body = self.parse_block()?;
            clean &= body.is_some();
            let body_end = body.as_ref().map_or(arm_start, |body| body.span);
            if let (Some(label), Some(body)) = (label, body) {
                arms.push(AstSwitchStatementArm {
                    label,
                    body,
                    span: self.cover(arm_start, body_end)?,
                });
            }
            if self.eat(TokenKind::Comma).is_none() && !self.at(TokenKind::RightBrace) {
                self.error(
                    EXPECTED_TOKEN,
                    "expected `,` or `}` after the switch arm",
                    self.current().span(),
                );
                clean = false;
                self.recover_list(TokenKind::RightBrace);
            }
        }
        let close = self.expect(TokenKind::RightBrace, "expected `}` after switch arms");
        self.leave_depth();
        clean &= close.is_some();
        let end = close.map_or_else(|| self.previous_or_current_span(), Token::span);
        let span = self.cover(start, end)?;
        Ok(if clean {
            AstStatement::Switch(AstSwitchStatement {
                scrutinee,
                arms,
                span,
            })
        } else {
            AstStatement::Error(span)
        })
    }

    fn parse_switch_label(&mut self) -> Result<Option<AstSwitchLabel>, SourceError> {
        if let Some(else_token) = self.eat(TokenKind::KeywordElse) {
            return Ok(Some(AstSwitchLabel::Else(else_token.span())));
        }
        let mut patterns = vec![];
        loop {
            let Some(pattern) = self.parse_switch_pattern()? else {
                return Ok(None);
            };
            patterns.push(pattern);
            if self.at(TokenKind::FatArrow) {
                break;
            }
            if self.eat(TokenKind::Comma).is_none() {
                self.error(
                    EXPECTED_TOKEN,
                    "expected `,` or `=>` after switch pattern",
                    self.current().span(),
                );
                return Ok(None);
            }
        }
        Ok(Some(AstSwitchLabel::Patterns(patterns)))
    }

    fn parse_switch_pattern(&mut self) -> Result<Option<AstSwitchPattern>, SourceError> {
        let start = self.current().span();
        if self.at(TokenKind::Dot) {
            self.bump();
            let Some(variant) = self.expect_identifier("expected enum variant after `.`") else {
                return Ok(None);
            };
            let variant = AstName {
                span: variant.span(),
            };
            return Ok(Some(AstSwitchPattern {
                kind: AstSwitchPatternKind::EnumVariant { ty: None, variant },
                span: self.cover(start, variant.span)?,
            }));
        }
        if is_identifier_like(self.current().kind()) && self.nth_kind(1) == TokenKind::Dot {
            let ty = AstName {
                span: self.bump().span(),
            };
            self.bump();
            let Some(variant) = self.expect_identifier("expected enum variant after `.`") else {
                return Ok(None);
            };
            let variant = AstName {
                span: variant.span(),
            };
            return Ok(Some(AstSwitchPattern {
                kind: AstSwitchPatternKind::EnumVariant {
                    ty: Some(ty),
                    variant,
                },
                span: self.cover(start, variant.span)?,
            }));
        }
        let Some(min) = self.parse_signed_integer() else {
            self.error(
                EXPECTED_EXPRESSION,
                "expected an integer or enum switch pattern",
                self.current().span(),
            );
            return Ok(None);
        };
        if self.eat(TokenKind::Ellipsis).is_some() {
            let Some(max) = self.parse_signed_integer() else {
                self.error(
                    EXPECTED_EXPRESSION,
                    "expected an integer after `...`",
                    self.current().span(),
                );
                return Ok(None);
            };
            Ok(Some(AstSwitchPattern {
                span: self.cover(start, max.span)?,
                kind: AstSwitchPatternKind::IntegerRange { min, max },
            }))
        } else {
            Ok(Some(AstSwitchPattern {
                span: min.span,
                kind: AstSwitchPatternKind::Integer(min),
            }))
        }
    }

    fn parse_signed_integer(&mut self) -> Option<AstSignedInteger> {
        let minus = self.eat(TokenKind::Minus);
        if !self.at(TokenKind::DecimalInteger) {
            return None;
        }
        let digits = self.bump().span();
        let span = minus.map_or(digits, |minus| {
            self.cover(minus.span(), digits).unwrap_or(digits)
        });
        Some(AstSignedInteger {
            negative: minus.is_some(),
            digits,
            span,
        })
    }

    fn parse_while_statement(&mut self) -> Result<AstStatement, SourceError> {
        let start = self.bump().span();
        let mut clean = self
            .expect(TokenKind::LeftParenthesis, "expected `(` after `while`")
            .is_some();
        let condition = self.parse_expression()?;
        clean &= !expression_has_error(&condition);
        clean &= self
            .expect(
                TokenKind::RightParenthesis,
                "expected `)` after the loop condition",
            )
            .is_some();
        let Some(body) = self.parse_block()? else {
            return self.error_statement(start);
        };
        let span = self.cover(start, body.span)?;
        Ok(if clean {
            AstStatement::While(AstWhileStatement {
                condition,
                body,
                span,
            })
        } else {
            AstStatement::Error(span)
        })
    }

    fn parse_loop_control_statement(
        &mut self,
        is_break: bool,
    ) -> Result<AstStatement, SourceError> {
        let keyword = self.bump();
        let semicolon = self.expect(
            TokenKind::Semicolon,
            if is_break {
                "expected `;` after `break`"
            } else {
                "expected `;` after `continue`"
            },
        );
        let end = semicolon.map_or(keyword.span(), Token::span);
        let span = self.cover(keyword.span(), end)?;
        if semicolon.is_none() {
            self.note_item_boundary();
            return Ok(AstStatement::Error(span));
        }
        Ok(if is_break {
            AstStatement::Break(span)
        } else {
            AstStatement::Continue(span)
        })
    }

    fn parse_declaration(&mut self) -> Result<AstStatement, SourceError> {
        let keyword = self.bump();
        let kind = if keyword.kind() == TokenKind::KeywordConst {
            AstBindingKind::Const
        } else {
            AstBindingKind::Var
        };
        let Some(name_token) = self.expect_identifier("expected a binding name") else {
            return self.error_statement(keyword.span());
        };
        let name = AstName {
            span: name_token.span(),
        };
        let inferred = self.eat(TokenKind::ColonEqual).is_some();
        let mut clean = true;
        let ty = if inferred {
            None
        } else {
            clean &= self
                .expect(TokenKind::Colon, "expected `:` or `:=` after the binding name")
                .is_some();
            let ty = self.parse_value_type();
            clean &= ty.is_some();
            ty
        };

        let initializer = if inferred {
            Some(self.parse_expression()?)
        } else if self.eat(TokenKind::Equal).is_some() {
            Some(self.parse_expression()?)
        } else if kind == AstBindingKind::Const {
            clean = false;
            self.error(
                EXPECTED_CONST_INITIALIZER,
                "a `const` declaration requires `= expression`",
                self.current().span(),
            );
            None
        } else {
            None
        };
        if initializer.as_ref().is_some_and(expression_has_error) {
            clean = false;
        }
        let semicolon = self.expect(TokenKind::Semicolon, "expected `;` after the declaration");
        clean &= semicolon.is_some();
        let end = semicolon.map_or_else(|| self.previous_or_current_span(), Token::span);
        let span = self.cover(keyword.span(), end)?;
        if !inferred && ty.is_none() {
            return Ok(AstStatement::Error(span));
        }
        Ok(if clean {
            AstStatement::Declaration(AstDeclaration {
                kind,
                name,
                ty,
                initializer,
                span,
            })
        } else {
            AstStatement::Error(span)
        })
    }

    fn parse_destructuring_statement(&mut self) -> Result<AstStatement, SourceError> {
        let start = self.bump().span();
        let mut targets = vec![];
        let mut clean = true;
        while !self.at(TokenKind::Pipe) && !self.at(TokenKind::EndOfFile) {
            let target_start = self.current().span();
            let kind = match self.current().kind() {
                TokenKind::KeywordConst => {
                    self.bump();
                    AstDestructureTargetKind::Const
                }
                TokenKind::KeywordVar => {
                    self.bump();
                    AstDestructureTargetKind::Var
                }
                _ => AstDestructureTargetKind::Assign,
            };
            let name = self.expect_identifier("expected a destructuring target name");
            clean &= name.is_some();
            if let Some(name) = name {
                targets.push(AstDestructureTarget {
                    kind,
                    name: AstName { span: name.span() },
                    span: self.cover(target_start, name.span())?,
                });
            }
            if self.eat(TokenKind::Comma).is_some() {
                continue;
            }
            if !self.at(TokenKind::Pipe) {
                self.error(EXPECTED_TOKEN, "expected `,` or `|` after destructuring target", self.current().span());
                clean = false;
                self.recover_list(TokenKind::Pipe);
            }
        }
        clean &= !targets.is_empty();
        clean &= self.expect(TokenKind::Pipe, "expected `|` after destructuring targets").is_some();
        clean &= self.expect(TokenKind::LessEqual, "expected `<=` after destructuring targets").is_some();
        let value = self.parse_expression()?;
        clean &= !expression_has_error(&value);
        let semicolon = self.expect(TokenKind::Semicolon, "expected `;` after destructuring");
        clean &= semicolon.is_some();
        let end = semicolon.map_or(value.span, Token::span);
        let span = self.cover(start, end)?;
        Ok(if clean {
            AstStatement::Destructure(AstDestructuringStatement { targets, value, span })
        } else {
            AstStatement::Error(span)
        })
    }

    fn parse_assignment(&mut self) -> Result<AstStatement, SourceError> {
        let name_token = self.bump();
        let target = AstName {
            span: name_token.span(),
        };
        self.bump();
        let value = self.parse_expression()?;
        let semicolon = self.expect(TokenKind::Semicolon, "expected `;` after the assignment");
        let end = semicolon.map_or(value.span, Token::span);
        let span = self.cover(name_token.span(), end)?;
        if semicolon.is_none() || expression_has_error(&value) {
            self.note_item_boundary();
            return Ok(AstStatement::Error(span));
        }
        Ok(AstStatement::Assignment(AstAssignment {
            target,
            value,
            span,
        }))
    }

    fn parse_call_statement(&mut self) -> Result<AstStatement, SourceError> {
        let expression = self.parse_expression()?;
        let expression_span = expression.span;
        let AstExpressionKind::Call(call) = expression.kind else {
            self.error(
                EXPECTED_ASSIGNMENT_OR_CALL,
                "expected an assignment or call statement",
                expression_span,
            );
            self.recover_statement();
            return Ok(AstStatement::Error(
                self.cover(expression_span, self.previous_or_current_span())?,
            ));
        };
        let semicolon = self.expect(TokenKind::Semicolon, "expected `;` after the call");
        let end = semicolon.map_or(call.span, Token::span);
        let span = self.cover(call.span, end)?;
        if semicolon.is_none() {
            self.note_item_boundary();
            return Ok(AstStatement::Error(span));
        }
        Ok(AstStatement::Call(AstCallStatement { call, span }))
    }

    fn parse_unsafe_minecraft_statement(&mut self) -> Result<AstStatement, SourceError> {
        let start = self.bump().span();
        let mut clean = self
            .expect(
                TokenKind::KeywordMinecraft,
                "expected `minecraft` after `unsafe`",
            )
            .is_some();
        clean &= self
            .expect(
                TokenKind::LeftParenthesis,
                "expected `(` after `unsafe minecraft`",
            )
            .is_some();
        let command = if self.at(TokenKind::StringLiteral) {
            Some(self.bump().span())
        } else {
            self.error(
                EXPECTED_TOKEN,
                "expected one literal command string",
                self.current().span(),
            );
            clean = false;
            None
        };
        clean &= self
            .expect(
                TokenKind::RightParenthesis,
                "expected `)` after the command literal",
            )
            .is_some();
        let semicolon = self.expect(
            TokenKind::Semicolon,
            "expected `;` after the unsafe command",
        );
        clean &= semicolon.is_some();
        let end = semicolon.map_or_else(|| self.previous_or_current_span(), Token::span);
        let span = self.cover(start, end)?;
        let Some(command) = command else {
            self.recover_statement();
            return Ok(AstStatement::Error(span));
        };
        if clean {
            Ok(AstStatement::UnsafeMinecraft(AstUnsafeMinecraftStatement {
                command,
                span,
            }))
        } else {
            self.note_item_boundary();
            Ok(AstStatement::Error(span))
        }
    }

    fn parse_if_statement(&mut self) -> Result<AstStatement, SourceError> {
        let start = self.bump().span();
        let Some(first) = self.parse_if_arm(start)? else {
            return self.error_statement(start);
        };
        let mut arms = vec![first];
        let mut else_body = None;
        let mut end = arms[0].body.span;

        while self.eat(TokenKind::KeywordElse).is_some() {
            if let Some(if_token) = self.eat(TokenKind::KeywordIf) {
                if let Some(arm) = self.parse_if_arm(if_token.span())? {
                    end = arm.body.span;
                    arms.push(arm);
                } else {
                    return self.error_statement(start);
                }
            } else {
                let Some(body) = self.parse_block()? else {
                    return self.error_statement(start);
                };
                end = body.span;
                else_body = Some(body);
                break;
            }
        }
        Ok(AstStatement::If(AstIfStatement {
            arms,
            else_body,
            span: self.cover(start, end)?,
        }))
    }

    fn parse_run_statement(&mut self) -> Result<AstStatement, SourceError> {
        let start = self.bump().span();
        let mut modifiers = vec![];
        let mut clean = true;
        while let Some(dot) = self.eat(TokenKind::Dot) {
            let Some(name_token) = self.expect_identifier("expected a run modifier after `.`")
            else {
                clean = false;
                break;
            };
            let name = AstName {
                span: name_token.span(),
            };
            let callee = AstExpression {
                kind: AstExpressionKind::Name(name),
                span: name.span,
            };
            let (call, call_clean) = self.parse_call_after_callee(callee)?;
            clean &= call_clean;
            modifiers.push(AstRunModifier {
                name,
                arguments: call.arguments,
                span: self.cover(dot.span(), call.span)?,
            });
        }

        let capture = if self.eat(TokenKind::Pipe).is_some() {
            let capture = self
                .expect_identifier("expected an executor capture name after `|`")
                .map(|token| AstName { span: token.span() });
            clean &= capture.is_some();
            clean &= self
                .expect(TokenKind::Pipe, "expected `|` after the executor capture")
                .is_some();
            capture
        } else {
            None
        };

        let Some(body) = self.parse_block()? else {
            return self.error_statement(start);
        };
        let span = self.cover(start, body.span)?;
        if clean {
            Ok(AstStatement::Run(AstRunStatement {
                modifiers,
                capture,
                body,
                span,
            }))
        } else {
            Ok(AstStatement::Error(span))
        }
    }

    fn parse_if_arm(&mut self, start: Span) -> Result<Option<AstIfArm>, SourceError> {
        let mut clean = self
            .expect(TokenKind::LeftParenthesis, "expected `(` after `if`")
            .is_some();
        let condition = self.parse_expression()?;
        clean &= !expression_has_error(&condition);
        clean &= self
            .expect(
                TokenKind::RightParenthesis,
                "expected `)` after the condition",
            )
            .is_some();
        let Some(body) = self.parse_block()? else {
            return Ok(None);
        };
        if clean {
            Ok(Some(AstIfArm {
                span: self.cover(start, body.span)?,
                condition,
                body,
            }))
        } else {
            Ok(None)
        }
    }

    fn parse_return_statement(&mut self) -> Result<AstStatement, SourceError> {
        let start = self.bump().span();
        let value = if self.at(TokenKind::Semicolon) {
            None
        } else if self.at_expression_start() {
            Some(self.parse_expression()?)
        } else {
            None
        };
        let semicolon = self.expect(TokenKind::Semicolon, "expected `;` after `return`");
        let end = semicolon.map_or_else(
            || value.as_ref().map_or(start, |expression| expression.span),
            Token::span,
        );
        let span = self.cover(start, end)?;
        if semicolon.is_none() || value.as_ref().is_some_and(expression_has_error) {
            self.note_item_boundary();
            return Ok(AstStatement::Error(span));
        }
        Ok(AstStatement::Return(AstReturnStatement { value, span }))
    }

    fn parse_expression(&mut self) -> Result<AstExpression, SourceError> {
        let left = self.parse_wrapping_arithmetic()?;
        let Some(op) = comparison_op(self.current().kind()) else {
            return Ok(left);
        };
        self.bump();
        let right = self.parse_wrapping_arithmetic()?;
        let span = self.cover(left.span, right.span)?;
        let mut expression = AstExpression {
            kind: AstExpressionKind::Compare {
                op,
                left: Box::new(left),
                right: Box::new(right),
            },
            span,
        };
        if comparison_op(self.current().kind()).is_some() {
            self.error(
                CHAINED_COMPARISON,
                "comparison operators cannot be chained",
                self.current().span(),
            );
            let start = expression.span;
            while comparison_op(self.current().kind()).is_some() {
                self.bump();
                let tail = self.parse_wrapping_arithmetic()?;
                expression.span = self.cover(start, tail.span)?;
            }
            expression.kind = AstExpressionKind::Error;
        }
        Ok(expression)
    }

    fn parse_wrapping_arithmetic(&mut self) -> Result<AstExpression, SourceError> {
        let mut expression = self.parse_prefix()?;
        loop {
            let op = match self.current().kind() {
                TokenKind::PlusPercent => AstWrappingArithmeticOp::Add,
                TokenKind::MinusPercent => AstWrappingArithmeticOp::Subtract,
                _ => break,
            };
            self.bump();
            let right = self.parse_prefix()?;
            let span = self.cover(expression.span, right.span)?;
            expression = AstExpression {
                kind: AstExpressionKind::WrappingArithmetic {
                    op,
                    left: Box::new(expression),
                    right: Box::new(right),
                },
                span,
            };
        }
        Ok(expression)
    }

    fn parse_prefix(&mut self) -> Result<AstExpression, SourceError> {
        if let Some(bang) = self.eat(TokenKind::Bang) {
            if !self.enter_depth(bang.span()) {
                return Ok(Self::error_expression(bang.span()));
            }
            let operand = self.parse_prefix()?;
            self.leave_depth();
            return Ok(AstExpression {
                span: self.cover(bang.span(), operand.span)?,
                kind: AstExpressionKind::Not(Box::new(operand)),
            });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<AstExpression, SourceError> {
        // The loop builds a left-nested AST even though parsing itself is iterative.
        // Retain each completed wrapper in `depth` until the whole spine is done so
        // later checking and recursive drop inherit the same reviewed stack bound.
        let incoming_depth = self.depth;
        let result = (|| {
            let mut expression = self.parse_primary()?;
            loop {
                if let Some(dot) = self.eat(TokenKind::Dot) {
                    if !self.enter_depth(dot.span()) {
                        expression.span = self.cover(expression.span, dot.span())?;
                        expression.kind = AstExpressionKind::Error;
                        break;
                    }
                    let Some(member_token) =
                        self.expect_identifier("expected a member name after `.`")
                    else {
                        expression.span = self.cover(expression.span, dot.span())?;
                        expression.kind = AstExpressionKind::Error;
                        break;
                    };
                    let member = AstName {
                        span: member_token.span(),
                    };
                    expression = AstExpression {
                        span: self.cover(expression.span, member.span)?,
                        kind: AstExpressionKind::Member {
                            receiver: Box::new(expression),
                            dot: dot.span(),
                            member,
                        },
                    };
                    continue;
                }
                if self.at(TokenKind::LeftParenthesis) {
                    let open = self.current();
                    if !self.enter_depth(open.span()) {
                        expression.span = self.cover(expression.span, open.span())?;
                        expression.kind = AstExpressionKind::Error;
                        break;
                    }
                    let (call, clean) = self.parse_call_after_callee(expression)?;
                    let span = call.span;
                    expression = AstExpression {
                        kind: if clean {
                            AstExpressionKind::Call(call)
                        } else {
                            AstExpressionKind::Error
                        },
                        span,
                    };
                    continue;
                }
                if let Some(open) = self.eat(TokenKind::LeftBracket) {
                    if !self.enter_depth(open.span()) {
                        expression.kind = AstExpressionKind::Error;
                        break;
                    }
                    let index = self.expect(
                        TokenKind::DecimalInteger,
                        "expected a decimal component index",
                    );
                    let close = self.expect(TokenKind::RightBracket, "expected `]` after index");
                    self.leave_depth();
                    let Some(index) = index else {
                        expression.kind = AstExpressionKind::Error;
                        break;
                    };
                    let end = close.map_or(index.span(), Token::span);
                    let span = self.cover(expression.span, end)?;
                    expression = AstExpression {
                        kind: if close.is_some() {
                            AstExpressionKind::Index {
                                aggregate: Box::new(expression),
                                index: index.span(),
                            }
                        } else {
                            AstExpressionKind::Error
                        },
                        span,
                    };
                    continue;
                }
                if self.at(TokenKind::LeftBrace) {
                    let AstExpressionKind::Name(ty) = &expression.kind else {
                        break;
                    };
                    let (literal, clean) = self.parse_struct_literal(*ty)?;
                    let span = literal.span;
                    expression = AstExpression {
                        kind: if clean {
                            AstExpressionKind::StructLiteral(literal)
                        } else {
                            AstExpressionKind::Error
                        },
                        span,
                    };
                    continue;
                }
                break;
            }
            Ok(expression)
        })();
        self.depth = incoming_depth;
        result
    }

    fn parse_struct_literal(
        &mut self,
        ty: AstName,
    ) -> Result<(AstStructLiteral, bool), SourceError> {
        self.bump();
        let mut fields = vec![];
        let mut clean = true;
        while !self.at(TokenKind::RightBrace) && !self.at(TokenKind::EndOfFile) {
            let start = self.current().span();
            clean &= self
                .expect(TokenKind::Dot, "expected `.` before a struct field")
                .is_some();
            let name = self.expect_identifier("expected a struct field name");
            clean &= name.is_some();
            clean &= self
                .expect(TokenKind::Equal, "expected `=` after the struct field name")
                .is_some();
            let value = self.parse_expression()?;
            clean &= !expression_has_error(&value);
            if let Some(name) = name {
                fields.push(AstStructFieldInitializer {
                    name: AstName { span: name.span() },
                    span: self.cover(start, value.span)?,
                    value,
                });
            }
            if self.eat(TokenKind::Comma).is_some() {
                continue;
            }
            if !self.at(TokenKind::RightBrace) {
                self.error(
                    EXPECTED_TOKEN,
                    "expected `,` or `}` after the struct field value",
                    self.current().span(),
                );
                clean = false;
                self.recover_list(TokenKind::RightBrace);
            }
        }
        let close = self.expect(
            TokenKind::RightBrace,
            "expected `}` after the struct literal",
        );
        clean &= close.is_some();
        let end = close.map_or_else(|| self.previous_or_current_span(), Token::span);
        Ok((
            AstStructLiteral {
                ty,
                fields,
                span: self.cover(ty.span, end)?,
            },
            clean,
        ))
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one exhaustive primary parser preserves bounded recovery behavior"
    )]
    fn parse_primary(&mut self) -> Result<AstExpression, SourceError> {
        let token = self.current();
        match token.kind() {
            TokenKind::KeywordSwitch => self.parse_switch_expression(),
            TokenKind::Dot => {
                let dot = self.bump();
                if self.at(TokenKind::LeftBrace) {
                    return self.parse_inferred_struct_literal(dot.span());
                }
                let Some(variant) = self.expect_identifier("expected enum variant after `.`")
                else {
                    return Ok(Self::error_expression(dot.span()));
                };
                let name = AstName {
                    span: variant.span(),
                };
                Ok(AstExpression {
                    span: self.cover(dot.span(), variant.span())?,
                    kind: AstExpressionKind::InferredEnumLiteral(name),
                })
            }
            TokenKind::KeywordTrue | TokenKind::KeywordFalse => {
                self.bump();
                Ok(AstExpression {
                    kind: AstExpressionKind::Bool(token.kind() == TokenKind::KeywordTrue),
                    span: token.span(),
                })
            }
            TokenKind::DecimalInteger => {
                self.bump();
                Ok(AstExpression {
                    kind: AstExpressionKind::DecimalInteger(token.span()),
                    span: token.span(),
                })
            }
            TokenKind::DecimalNumber => {
                self.bump();
                Ok(AstExpression {
                    kind: AstExpressionKind::StaticDecimal {
                        sigil: None,
                        negative: false,
                        digits: Some(token.span()),
                    },
                    span: token.span(),
                })
            }
            TokenKind::Minus | TokenKind::Tilde | TokenKind::Caret => {
                let prefix = self.bump();
                let sigil = match prefix.kind() {
                    TokenKind::Tilde => Some(AstCoordinateSigil::Relative),
                    TokenKind::Caret => Some(AstCoordinateSigil::Local),
                    TokenKind::Minus => None,
                    _ => unreachable!(),
                };
                let negative = if prefix.kind() == TokenKind::Minus {
                    true
                } else {
                    self.eat(TokenKind::Minus).is_some()
                };
                let digits = if matches!(
                    self.current().kind(),
                    TokenKind::DecimalInteger | TokenKind::DecimalNumber
                ) {
                    Some(self.bump().span())
                } else {
                    None
                };
                if sigil.is_none() && digits.is_none() {
                    self.error(
                        EXPECTED_EXPRESSION,
                        "expected decimal digits after `-`",
                        prefix.span(),
                    );
                    return Ok(Self::error_expression(prefix.span()));
                }
                let end = digits.unwrap_or(prefix.span());
                Ok(AstExpression {
                    kind: AstExpressionKind::StaticDecimal {
                        sigil,
                        negative,
                        digits,
                    },
                    span: self.cover(prefix.span(), end)?,
                })
            }
            TokenKind::StringLiteral => {
                self.bump();
                Ok(AstExpression {
                    kind: AstExpressionKind::StringLiteral(token.span()),
                    span: token.span(),
                })
            }
            kind if is_identifier_like(kind) => {
                self.bump();
                let name = AstName { span: token.span() };
                Ok(AstExpression {
                    kind: AstExpressionKind::Name(name),
                    span: token.span(),
                })
            }
            TokenKind::LeftParenthesis => {
                let open = self.bump();
                if !self.enter_depth(open.span()) {
                    return Ok(Self::error_expression(open.span()));
                }
                let mut expression = self.parse_expression()?;
                let close = self.expect(
                    TokenKind::RightParenthesis,
                    "expected `)` after the expression",
                );
                self.leave_depth();
                if let Some(close) = close {
                    expression.span = self.cover(open.span(), close.span())?;
                } else {
                    expression.span = self.cover(open.span(), expression.span)?;
                    expression.kind = AstExpressionKind::Error;
                    self.note_item_boundary();
                }
                Ok(expression)
            }
            _ => {
                self.error(EXPECTED_EXPRESSION, "expected an expression", token.span());
                if matches!(
                    token.kind(),
                    TokenKind::KeywordFn | TokenKind::KeywordPub | TokenKind::KeywordExport
                ) {
                    self.item_boundary = true;
                }
                if !matches!(
                    token.kind(),
                    TokenKind::Semicolon
                        | TokenKind::Comma
                        | TokenKind::RightParenthesis
                        | TokenKind::RightBrace
                        | TokenKind::KeywordConst
                        | TokenKind::KeywordVar
                        | TokenKind::KeywordIf
                        | TokenKind::KeywordReturn
                        | TokenKind::KeywordRun
                        | TokenKind::KeywordUnsafe
                        | TokenKind::KeywordFn
                        | TokenKind::KeywordPub
                        | TokenKind::KeywordExport
                        | TokenKind::EndOfFile
                ) {
                    self.bump();
                }
                Ok(Self::error_expression(token.span()))
            }
        }
    }

    fn parse_inferred_struct_literal(&mut self, dot: Span) -> Result<AstExpression, SourceError> {
        let open = self.bump();
        if !self.enter_depth(open.span()) {
            return Ok(Self::error_expression(dot));
        }
        let named = self.at(TokenKind::Dot)
            && self.nth_kind(1) == TokenKind::Identifier
            && self.nth_kind(2) == TokenKind::Equal;
        let mut fields = vec![];
        let mut values = vec![];
        let mut clean = true;
        while !self.at(TokenKind::RightBrace) && !self.at(TokenKind::EndOfFile) {
            if named {
                let start = self.current().span();
                clean &= self.expect(TokenKind::Dot, "expected `.` before field name").is_some();
                let name = self.expect_identifier("expected an inferred struct field name");
                clean &= name.is_some();
                clean &= self.expect(TokenKind::Equal, "expected `=` after field name").is_some();
                let value = self.parse_expression()?;
                clean &= !expression_has_error(&value);
                if let Some(name) = name {
                    fields.push(AstStructFieldInitializer {
                        name: AstName { span: name.span() },
                        span: self.cover(start, value.span)?,
                        value,
                    });
                }
            } else {
                let value = self.parse_expression()?;
                clean &= !expression_has_error(&value);
                values.push(value);
            }
            if self.eat(TokenKind::Comma).is_some() {
                continue;
            }
            if !self.at(TokenKind::RightBrace) {
                self.error(
                    EXPECTED_TOKEN,
                    "expected `,` or `}` after inferred struct entry",
                    self.current().span(),
                );
                clean = false;
                self.recover_list(TokenKind::RightBrace);
            }
        }
        let close = self.expect(TokenKind::RightBrace, "expected `}` after inferred struct literal");
        self.leave_depth();
        clean &= close.is_some();
        let end = close.map_or_else(|| self.previous_or_current_span(), Token::span);
        let span = self.cover(dot, end)?;
        Ok(AstExpression {
            kind: if clean {
                AstExpressionKind::InferredStructLiteral(AstInferredStructLiteral {
                    entries: if named {
                        AstInferredStructEntries::Named(fields)
                    } else {
                        AstInferredStructEntries::Positional(values)
                    },
                    span,
                })
            } else {
                AstExpressionKind::Error
            },
            span,
        })
    }

    fn parse_switch_expression(&mut self) -> Result<AstExpression, SourceError> {
        let start = self.bump().span();
        let mut clean = self
            .expect(TokenKind::LeftParenthesis, "expected `(` after `switch`")
            .is_some();
        let scrutinee = self.parse_expression()?;
        clean &= !expression_has_error(&scrutinee);
        clean &= self
            .expect(
                TokenKind::RightParenthesis,
                "expected `)` after switch scrutinee",
            )
            .is_some();
        let Some(open) = self.expect(TokenKind::LeftBrace, "expected `{` to begin switch arms")
        else {
            return Ok(Self::error_expression(self.cover(start, scrutinee.span)?));
        };
        if !self.enter_depth(open.span()) {
            return Ok(Self::error_expression(self.cover(start, open.span())?));
        }
        let mut arms = vec![];
        while !self.at(TokenKind::RightBrace) && !self.at(TokenKind::EndOfFile) {
            let arm_start = self.current().span();
            let label = self.parse_switch_label()?;
            clean &= label.is_some();
            clean &= self
                .expect(TokenKind::FatArrow, "expected `=>` after switch patterns")
                .is_some();
            let body = self.parse_expression()?;
            clean &= !expression_has_error(&body);
            let arm_end = body.span;
            if let Some(label) = label {
                arms.push(AstSwitchExpressionArm {
                    label,
                    body,
                    span: self.cover(arm_start, arm_end)?,
                });
            }
            if self.eat(TokenKind::Comma).is_none() && !self.at(TokenKind::RightBrace) {
                self.error(
                    EXPECTED_TOKEN,
                    "expected `,` or `}` after the switch arm",
                    self.current().span(),
                );
                clean = false;
                self.recover_list(TokenKind::RightBrace);
            }
        }
        let close = self.expect(TokenKind::RightBrace, "expected `}` after switch arms");
        self.leave_depth();
        clean &= close.is_some();
        let end = close.map_or_else(|| self.previous_or_current_span(), Token::span);
        let span = self.cover(start, end)?;
        Ok(AstExpression {
            kind: if clean {
                AstExpressionKind::Switch(AstSwitchExpression {
                    scrutinee: Box::new(scrutinee),
                    arms,
                    span,
                })
            } else {
                AstExpressionKind::Error
            },
            span,
        })
    }

    fn parse_call_after_callee(
        &mut self,
        callee: AstExpression,
    ) -> Result<(AstCall, bool), SourceError> {
        let Some(open) = self.expect(TokenKind::LeftParenthesis, "expected `(` after the callee")
        else {
            let span = callee.span;
            return Ok((
                AstCall {
                    callee: Box::new(callee),
                    arguments: vec![],
                    span,
                },
                false,
            ));
        };
        if !self.enter_depth(open.span()) {
            let span = self.cover(callee.span, open.span())?;
            return Ok((
                AstCall {
                    callee: Box::new(callee),
                    arguments: vec![],
                    span,
                },
                false,
            ));
        }
        let mut arguments = vec![];
        let mut clean = true;
        let mut closing_error_reported = false;
        if !self.at(TokenKind::RightParenthesis) {
            loop {
                let argument = self.parse_expression()?;
                clean &= !expression_has_error(&argument);
                arguments.push(argument);
                if self.eat(TokenKind::Comma).is_some() {
                    if self.at(TokenKind::RightParenthesis) {
                        break;
                    }
                } else if self.at(TokenKind::RightParenthesis) {
                    break;
                } else {
                    self.error(
                        EXPECTED_TOKEN,
                        "expected `,` or `)` after the call argument",
                        self.current().span(),
                    );
                    clean = false;
                    closing_error_reported = true;
                    self.recover_call_arguments();
                    if self.eat(TokenKind::Comma).is_some() && !self.at(TokenKind::RightParenthesis)
                    {
                        continue;
                    }
                    break;
                }
            }
        }
        let close = if self.at(TokenKind::RightParenthesis) {
            Some(self.bump())
        } else {
            if !closing_error_reported {
                self.error(
                    EXPECTED_TOKEN,
                    "expected `)` after call arguments",
                    self.current().span(),
                );
            }
            clean = false;
            None
        };
        self.leave_depth();
        let end = close.map_or_else(|| self.previous_or_current_span(), Token::span);
        let span = self.cover(callee.span, end)?;
        Ok((
            AstCall {
                callee: Box::new(callee),
                arguments,
                span,
            },
            clean,
        ))
    }

    fn error_statement(&mut self, start: Span) -> Result<AstStatement, SourceError> {
        if self.at_function_item_start() {
            self.item_boundary = true;
        } else if !self.item_boundary {
            self.recover_statement();
        }
        Ok(AstStatement::Error(
            self.cover(start, self.previous_or_current_span())?,
        ))
    }

    const fn error_expression(span: Span) -> AstExpression {
        AstExpression {
            kind: AstExpressionKind::Error,
            span,
        }
    }

    fn expect_identifier(&mut self, message: &'static str) -> Option<Token> {
        if is_identifier_like(self.current().kind()) {
            Some(self.bump())
        } else {
            self.error(EXPECTED_IDENTIFIER, message, self.current().span());
            None
        }
    }

    fn expect(&mut self, kind: TokenKind, message: &'static str) -> Option<Token> {
        if self.at(kind) {
            Some(self.bump())
        } else {
            self.error(EXPECTED_TOKEN, message, self.current().span());
            None
        }
    }

    fn eat(&mut self, kind: TokenKind) -> Option<Token> {
        self.at(kind).then(|| self.bump())
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.current().kind() == kind
    }

    fn at_expression_start(&self) -> bool {
        matches!(
            self.current().kind(),
            TokenKind::Bang
                | TokenKind::LeftParenthesis
                | TokenKind::KeywordTrue
                | TokenKind::KeywordFalse
                | TokenKind::DecimalInteger
                | TokenKind::DecimalNumber
                | TokenKind::Minus
                | TokenKind::Tilde
                | TokenKind::Caret
                | TokenKind::StringLiteral
                | TokenKind::Identifier
                | TokenKind::KeywordRun
                | TokenKind::KeywordSwitch
                | TokenKind::Dot
        )
    }

    fn note_item_boundary(&mut self) {
        if self.at_function_item_start() {
            self.item_boundary = true;
        }
    }

    fn at_function_item_start(&self) -> bool {
        matches!(
            self.current().kind(),
            TokenKind::KeywordFn | TokenKind::KeywordPub | TokenKind::KeywordExport
        )
    }

    fn nth_kind(&self, distance: usize) -> TokenKind {
        self.tokens
            .get(self.index.saturating_add(distance))
            .or_else(|| self.tokens.last())
            .map_or(TokenKind::EndOfFile, |token| token.kind())
    }

    fn current(&self) -> Token {
        self.tokens
            .get(self.index)
            .or_else(|| self.tokens.last())
            .copied()
            .expect("lexer-produced buffers contain EOF")
    }

    fn bump(&mut self) -> Token {
        let token = self.current();
        if token.kind() != TokenKind::EndOfFile {
            self.index += 1;
        }
        token
    }

    fn previous_or_current_span(&self) -> Span {
        self.index
            .checked_sub(1)
            .and_then(|index| self.tokens.get(index))
            .copied()
            .unwrap_or_else(|| self.current())
            .span()
    }

    fn cover(&self, start: Span, end: Span) -> Result<Span, SourceError> {
        debug_assert_eq!(start.file(), self.file);
        debug_assert_eq!(end.file(), self.file);
        self.sources.span(self.file, start.start(), end.end())
    }

    fn error(&mut self, code: &'static str, message: impl Into<String>, span: Span) {
        if self.truncation.is_some() {
            return;
        }
        if self.findings.len() < self.limits.max_diagnostics() {
            self.findings.push(PendingDiagnostic {
                code,
                message: message.into(),
                span,
            });
        } else {
            self.truncation = Some(PendingDiagnostic {
                code: TRUNCATED,
                message: "additional parser diagnostics were omitted".to_owned(),
                span,
            });
        }
    }

    fn enter_depth(&mut self, span: Span) -> bool {
        if self.depth < self.limits.max_syntax_depth() {
            self.depth += 1;
            true
        } else {
            if self.truncation.is_none() {
                self.truncation = Some(PendingDiagnostic {
                    code: TRUNCATED,
                    message: "parsing stopped at the syntax nesting limit".to_owned(),
                    span,
                });
            }
            false
        }
    }

    fn leave_depth(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    fn recover_item(&mut self) {
        while !matches!(
            self.current().kind(),
            TokenKind::KeywordConst
                | TokenKind::KeywordFn
                | TokenKind::KeywordPub
                | TokenKind::KeywordExport
                | TokenKind::EndOfFile
        ) {
            self.bump();
        }
    }

    fn recover_item_body(&mut self) {
        while !matches!(
            self.current().kind(),
            TokenKind::LeftBrace
                | TokenKind::KeywordConst
                | TokenKind::KeywordFn
                | TokenKind::KeywordPub
                | TokenKind::KeywordExport
                | TokenKind::EndOfFile
        ) {
            self.bump();
        }
        if self.at(TokenKind::LeftBrace) {
            self.bump();
            self.recover_block_body();
        } else {
            self.note_item_boundary();
        }
    }

    fn recover_block_body(&mut self) {
        let mut braces = 1usize;
        while !self.at(TokenKind::EndOfFile) {
            if self.at_function_item_start() {
                self.item_boundary = true;
                break;
            }
            match self.bump().kind() {
                TokenKind::LeftBrace => braces = braces.saturating_add(1),
                TokenKind::RightBrace => {
                    braces -= 1;
                    if braces == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
        self.note_item_boundary();
    }

    fn recover_statement(&mut self) {
        while !matches!(
            self.current().kind(),
            TokenKind::Semicolon
                | TokenKind::RightBrace
                | TokenKind::KeywordConst
                | TokenKind::KeywordVar
                | TokenKind::KeywordIf
                | TokenKind::KeywordReturn
                | TokenKind::KeywordRun
                | TokenKind::KeywordUnsafe
                | TokenKind::KeywordFn
                | TokenKind::KeywordPub
                | TokenKind::KeywordExport
                | TokenKind::Identifier
                | TokenKind::EndOfFile
        ) {
            self.bump();
        }
        self.eat(TokenKind::Semicolon);
        self.note_item_boundary();
    }

    fn recover_list(&mut self, closing: TokenKind) {
        while !self.at(TokenKind::Comma)
            && !self.at(closing)
            && !self.at(TokenKind::LeftBrace)
            && !self.at(TokenKind::RightBrace)
            && !self.at(TokenKind::KeywordFn)
            && !self.at(TokenKind::KeywordPub)
            && !self.at(TokenKind::KeywordExport)
            && !self.at(TokenKind::KeywordRun)
            && !self.at(TokenKind::KeywordUnsafe)
            && !self.at(TokenKind::EndOfFile)
        {
            self.bump();
        }
        self.note_item_boundary();
    }

    fn recover_call_arguments(&mut self) {
        while !matches!(
            self.current().kind(),
            TokenKind::Comma
                | TokenKind::RightParenthesis
                | TokenKind::Semicolon
                | TokenKind::RightBrace
                | TokenKind::KeywordConst
                | TokenKind::KeywordVar
                | TokenKind::KeywordIf
                | TokenKind::KeywordReturn
                | TokenKind::KeywordRun
                | TokenKind::KeywordUnsafe
                | TokenKind::KeywordFn
                | TokenKind::KeywordPub
                | TokenKind::KeywordExport
                | TokenKind::EndOfFile
        ) {
            self.bump();
        }
        self.note_item_boundary();
    }

    fn ensure_progress(&mut self, before: usize) {
        if self.index == before && !self.at(TokenKind::EndOfFile) {
            self.bump();
        }
    }
}

const fn comparison_op(kind: TokenKind) -> Option<AstComparisonOp> {
    match kind {
        TokenKind::EqualEqual => Some(AstComparisonOp::Equal),
        TokenKind::BangEqual => Some(AstComparisonOp::NotEqual),
        TokenKind::Less => Some(AstComparisonOp::Less),
        TokenKind::LessEqual => Some(AstComparisonOp::LessEqual),
        TokenKind::Greater => Some(AstComparisonOp::Greater),
        TokenKind::GreaterEqual => Some(AstComparisonOp::GreaterEqual),
        _ => None,
    }
}

const fn is_identifier_like(kind: TokenKind) -> bool {
    matches!(kind, TokenKind::Identifier | TokenKind::KeywordRun)
}

fn expression_has_error(expression: &AstExpression) -> bool {
    match &expression.kind {
        AstExpressionKind::Error => true,
        AstExpressionKind::Member { receiver, .. } => expression_has_error(receiver),
        AstExpressionKind::Call(call) => {
            expression_has_error(&call.callee) || call.arguments.iter().any(expression_has_error)
        }
        AstExpressionKind::StructLiteral(literal) => literal
            .fields
            .iter()
            .any(|field| expression_has_error(&field.value)),
        AstExpressionKind::Not(operand) => expression_has_error(operand),
        AstExpressionKind::WrappingArithmetic { left, right, .. }
        | AstExpressionKind::Compare { left, right, .. } => {
            expression_has_error(left) || expression_has_error(right)
        }
        AstExpressionKind::Switch(switch) => {
            expression_has_error(&switch.scrutinee)
                || switch
                    .arms
                    .iter()
                    .any(|arm| expression_has_error(&arm.body))
        }
        AstExpressionKind::Bool(_)
        | AstExpressionKind::DecimalInteger(_)
        | AstExpressionKind::StaticDecimal { .. }
        | AstExpressionKind::StringLiteral(_)
        | AstExpressionKind::InferredEnumLiteral(_)
        | AstExpressionKind::InferredStructLiteral(_)
        | AstExpressionKind::Index { .. }
        | AstExpressionKind::Name(_) => false,
    }
}

fn materialize_diagnostics(
    sources: &mut SourceContext,
    findings: Vec<PendingDiagnostic>,
    truncation: Option<PendingDiagnostic>,
) -> Result<Option<Diagnostics>, OriginError> {
    let mut diagnostics = Vec::with_capacity(findings.len() + usize::from(truncation.is_some()));
    for finding in findings.into_iter().chain(truncation) {
        let origin = sources.add_origin(Origin::Source(finding.span))?;
        diagnostics.push(Diagnostic::new(finding.code, finding.message, origin));
    }
    Ok(Diagnostics::from_findings(diagnostics))
}

#[cfg(test)]
mod tests {
    use super::{CHAINED_COMPARISON, ParseOutput, TRUNCATED, parse};
    use crate::frontend::FrontendLimits;
    use crate::frontend::ast::{
        AstBindingKind, AstExpressionKind, AstFunctionVisibility, AstResultTypeKind, AstStatement,
        AstValueTypeKind, dump,
    };
    use crate::frontend::lexer::lex;
    use crate::source::{FileId, SourceContext};

    fn parse_text_with_limits(
        text: &str,
        limits: FrontendLimits,
    ) -> (SourceContext, FileId, ParseOutput) {
        let mut sources = SourceContext::new();
        let file = sources.add_file("test.mdl", text).unwrap();
        let lexed = lex(&mut sources, file, limits).unwrap();
        assert_eq!(
            lexed.diagnostics(),
            None,
            "parser fixtures must be lexically clean"
        );
        assert!(!lexed.is_truncated());
        let parsed = parse(&mut sources, file, lexed.tokens(), limits).unwrap();
        (sources, file, parsed)
    }

    fn parse_text(text: &str) -> (SourceContext, FileId, ParseOutput) {
        parse_text_with_limits(text, FrontendLimits::DEFAULT)
    }

    fn codes(output: &ParseOutput) -> Vec<&'static str> {
        output
            .diagnostics()
            .map_or(&[][..], |diagnostics| diagnostics.findings())
            .iter()
            .map(crate::diagnostic::Diagnostic::code)
            .collect()
    }

    fn spelling(sources: &SourceContext, span: crate::source::Span) -> &str {
        sources.files().slice(span).unwrap()
    }

    #[test]
    fn parses_the_complete_first_scalar_shape() {
        let text = r"
fn choose(condition: Bool, left: Int32, right: Int32,) -> Int32 {
    var selected: Int32;
    if (condition) {
        selected = left;
    } else {
        selected = right;
    }

    return selected;
}

fn forward(condition: Bool, left: Int32, right: Int32) -> Int32 {
    return choose(condition, left, right,);
}

fn notify() -> Void {
    forward(true, 1, 2);
    return;
}
";
        let (sources, file, output) = parse_text(text);
        assert_eq!(output.diagnostics(), None);
        assert!(!output.is_truncated());
        assert_eq!(output.module().span.file(), file);
        assert_eq!(output.module().span.start(), 0);
        assert_eq!(
            output.module().span.end(),
            u32::try_from(text.len()).unwrap()
        );

        let functions = &output.module().functions;
        assert_eq!(functions.len(), 3);
        assert_eq!(spelling(&sources, functions[0].name.span), "choose");
        assert_eq!(functions[0].parameters.len(), 3);
        assert_eq!(functions[0].parameters[0].ty.kind, AstValueTypeKind::Bool);
        assert_eq!(
            functions[0].result.unwrap().kind,
            AstResultTypeKind::Value(AstValueTypeKind::Int32)
        );
        assert!(matches!(
            &functions[0].body.statements[..],
            [
                AstStatement::Declaration(_),
                AstStatement::If(_),
                AstStatement::Return(_)
            ]
        ));
        let AstStatement::Declaration(declaration) = &functions[0].body.statements[0] else {
            unreachable!();
        };
        assert_eq!(declaration.kind, AstBindingKind::Var);
        assert!(declaration.initializer.is_none());
        let AstStatement::If(conditional) = &functions[0].body.statements[1] else {
            unreachable!();
        };
        assert_eq!(conditional.arms.len(), 1);
        assert!(conditional.else_body.is_some());

        let AstStatement::Return(return_statement) = &functions[1].body.statements[0] else {
            unreachable!();
        };
        let AstExpressionKind::Call(call) = &return_statement.value.as_ref().unwrap().kind else {
            unreachable!();
        };
        assert_eq!(spelling(&sources, call.callee.span), "choose");
        assert_eq!(call.arguments.len(), 3);
        assert_eq!(functions[2].result.unwrap().kind, AstResultTypeKind::Void);
        assert_eq!(
            spelling(&sources, functions[0].result.unwrap().span),
            "-> Int32"
        );
        assert!(matches!(
            &functions[2].body.statements[..],
            [AstStatement::Call(_), AstStatement::Return(_)]
        ));
        let AstStatement::Call(call) = &functions[2].body.statements[0] else {
            unreachable!();
        };
        assert_eq!(spelling(&sources, call.call.span), "forward(true, 1, 2)");
        assert_eq!(spelling(&sources, call.span), "forward(true, 1, 2);");
    }

    #[test]
    fn parses_imports_visibility_and_one_uniform_postfix_spine() {
        let text = r#"
const cells = import("cells-api");
pub fn helper(value: Int32) -> Int32 { return value; }
export fn run(value: Int32) -> Int32 {
    return cells.normalize(helper(value));
}
"#;
        let (sources, _, output) = parse_text(text);
        assert_eq!(output.diagnostics(), None);
        let module = output.module();
        assert_eq!(module.imports.len(), 1);
        assert_eq!(spelling(&sources, module.imports[0].binding.span), "cells");
        assert_eq!(
            spelling(&sources, module.imports[0].dependency),
            "\"cells-api\""
        );
        assert_eq!(
            module.functions[0].visibility,
            AstFunctionVisibility::Public
        );
        assert_eq!(
            module.functions[1].visibility,
            AstFunctionVisibility::Export
        );

        let AstStatement::Return(return_statement) = &module.functions[1].body.statements[0] else {
            unreachable!();
        };
        let AstExpressionKind::Call(call) = &return_statement.value.as_ref().unwrap().kind else {
            unreachable!();
        };
        let AstExpressionKind::Member {
            receiver, member, ..
        } = &call.callee.kind
        else {
            unreachable!();
        };
        assert_eq!(spelling(&sources, member.span), "normalize");
        assert!(matches!(receiver.kind, AstExpressionKind::Name(_)));
        assert!(matches!(call.arguments[0].kind, AstExpressionKind::Call(_)));
    }

    #[test]
    fn distinguishes_names_calls_and_assignments_with_lookahead() {
        let (_, _, output) =
            parse_text("fn f(x: Int32) -> Int32 { var y: Int32 = x; y = f(y); f(y); return y; }");
        assert_eq!(output.diagnostics(), None);
        let statements = &output.module().functions[0].body.statements;
        assert!(matches!(
            &statements[..],
            [
                AstStatement::Declaration(_),
                AstStatement::Assignment(_),
                AstStatement::Call(_),
                AstStatement::Return(_)
            ]
        ));
        let AstStatement::Assignment(assignment) = &statements[1] else {
            unreachable!();
        };
        assert!(matches!(assignment.value.kind, AstExpressionKind::Call(_)));
        let AstStatement::Return(return_statement) = &statements[3] else {
            unreachable!();
        };
        assert!(matches!(
            return_statement.value.as_ref().unwrap().kind,
            AstExpressionKind::Name(_)
        ));
    }

    #[test]
    fn run_is_contextual_and_remains_an_ordinary_function_name() {
        let (_, _, output) = parse_text("fn run() {} fn caller() { run(); run {} }");
        assert_eq!(output.diagnostics(), None);
        assert!(matches!(
            &output.module().functions[1].body.statements[..],
            [AstStatement::Call(_), AstStatement::Run(_)]
        ));
    }

    #[test]
    fn parses_prefix_and_one_comparison_without_chaining() {
        let (_, _, output) = parse_text(
            "fn different(left: Int32, right: Int32) -> Bool { return !(left == right); }",
        );
        assert_eq!(output.diagnostics(), None);
        let AstStatement::Return(return_statement) =
            &output.module().functions[0].body.statements[0]
        else {
            unreachable!();
        };
        let AstExpressionKind::Not(operand) = &return_statement.value.as_ref().unwrap().kind else {
            unreachable!();
        };
        assert!(matches!(operand.kind, AstExpressionKind::Compare { .. }));
    }

    #[test]
    fn unsafe_minecraft_accepts_exactly_one_string_literal_statement() {
        let (sources, _, output) =
            parse_text(r#"fn raw() { unsafe minecraft("say hello"); return; }"#);
        assert_eq!(output.diagnostics(), None);
        let AstStatement::UnsafeMinecraft(statement) =
            &output.module().functions[0].body.statements[0]
        else {
            panic!("expected unsafe Minecraft statement");
        };
        assert_eq!(spelling(&sources, statement.command), r#""say hello""#);

        for malformed in [
            "fn raw() { unsafe minecraft(command); return; }",
            r#"fn raw() { unsafe minecraft("say a", "say b"); return; }"#,
            r#"fn raw() { unsafe minecraft("say a" "say b"); return; }"#,
        ] {
            let (_, _, output) = parse_text(malformed);
            assert!(output.diagnostics().is_some(), "{malformed}");
        }
    }

    #[test]
    fn parses_run_modifier_chain_capture_and_nested_block_as_one_statement() {
        let text = r#"fn scoped() {
    run.as(mc.entities(ArmorStand).with_tag("stage7").limit(1)) |speaker| {
        unsafe minecraft("say hello");
    }
}"#;
        let (sources, _, output) = parse_text(text);
        assert_eq!(output.diagnostics(), None);
        let AstStatement::Run(run) = &output.module().functions[0].body.statements[0] else {
            panic!("expected one structured run statement");
        };
        assert_eq!(run.modifiers.len(), 1);
        assert_eq!(spelling(&sources, run.modifiers[0].name.span), "as");
        assert_eq!(run.modifiers[0].arguments.len(), 1);
        assert_eq!(spelling(&sources, run.capture.unwrap().span), "speaker");
        assert!(matches!(
            run.body.statements.as_slice(),
            [AstStatement::UnsafeMinecraft(_)]
        ));
        assert_eq!(
            spelling(&sources, run.span),
            text[text.find("run.as").unwrap()..text.rfind('\n').unwrap()].trim_end()
        );
    }

    #[test]
    fn recovers_at_item_and_statement_boundaries() {
        let (sources, _, output) =
            parse_text("var top: Int32; fn first() { true; return; } fn second() { return; }");
        assert!(output.diagnostics().is_some());
        assert_eq!(output.module().functions.len(), 2);
        assert_eq!(
            spelling(&sources, output.module().functions[0].name.span),
            "first"
        );
        assert_eq!(
            spelling(&sources, output.module().functions[1].name.span),
            "second"
        );
        assert!(matches!(
            output.module().functions[0].body.statements[0],
            AstStatement::Error(_)
        ));
    }

    #[test]
    fn discards_a_malformed_function_and_keeps_the_next_item() {
        let (sources, _, output) =
            parse_text("fn (x: Int32) { if (true) { return; } } fn later() { return; }");
        assert!(output.diagnostics().is_some());
        assert_eq!(output.module().functions.len(), 1);
        assert_eq!(
            spelling(&sources, output.module().functions[0].name.span),
            "later"
        );
    }

    #[test]
    fn missing_parameter_list_open_does_not_consume_the_next_function() {
        let (sources, _, output) = parse_text("fn bad { return; } fn good() { return; }");
        assert!(output.diagnostics().is_some());
        assert_eq!(output.module().functions.len(), 1);
        assert_eq!(
            spelling(&sources, output.module().functions[0].name.span),
            "good"
        );
    }

    #[test]
    fn malformed_parameter_list_stops_at_body_and_preserves_the_next_function() {
        let (sources, _, output) = parse_text("fn bad(x: Int32 { return; } fn good() { return; }");
        assert!(output.diagnostics().is_some());
        assert_eq!(output.module().functions.len(), 1);
        assert_eq!(
            spelling(&sources, output.module().functions[0].name.span),
            "good"
        );
    }

    #[test]
    fn missing_function_brace_preserves_a_following_function_keyword() {
        let (sources, _, output) = parse_text("fn bad() { return; fn good() { return; }");
        assert!(output.diagnostics().is_some());
        assert_eq!(output.module().functions.len(), 1);
        assert_eq!(
            spelling(&sources, output.module().functions[0].name.span),
            "good"
        );
    }

    #[test]
    fn function_keyword_is_a_hard_recovery_boundary_at_every_nesting_level() {
        for text in [
            "fn bad() { if true) { return; } fn good() { return; }",
            "fn bad() { return fn good() { return; }",
            "fn (x: Int32) { return; fn good() { return; }",
        ] {
            let (sources, _, output) = parse_text(text);
            assert!(output.diagnostics().is_some(), "fixture must be malformed");
            assert_eq!(output.module().functions.len(), 1, "input: {text}");
            assert_eq!(
                spelling(&sources, output.module().functions[0].name.span),
                "good",
                "input: {text}"
            );
        }
    }

    #[test]
    fn call_recovery_preserves_a_following_statement_starter() {
        let (_, _, output) = parse_text("fn f() { notify(1 return; }");
        assert!(output.diagnostics().is_some());
        assert!(matches!(
            &output.module().functions[0].body.statements[..],
            [AstStatement::Error(_), AstStatement::Return(_)]
        ));
    }

    #[test]
    fn return_without_a_semicolon_preserves_a_following_if_statement() {
        let (_, _, output) = parse_text("fn f() { return if (true) { return; } }");
        assert!(output.diagnostics().is_some());
        assert!(matches!(
            &output.module().functions[0].body.statements[..],
            [AstStatement::Error(_), AstStatement::If(_)]
        ));
    }

    #[test]
    fn malformed_required_delimiters_produce_recovery_nodes() {
        let (sources, _, output) = parse_text("fn f(x: Int32) { f(x; return (x; }");
        assert!(output.diagnostics().is_some());
        assert!(matches!(
            &output.module().functions[0].body.statements[..],
            [AstStatement::Error(_), AstStatement::Error(_)]
        ));
        assert_eq!(
            dump(output.module(), &sources),
            "module @0..34\n  private function f @0..34\n    parameter x: Int32 @5..13\n    result Void (omitted)\n    block @15..34\n      error @17..21\n      error @22..32\n"
        );
    }

    #[test]
    fn ast_dump_is_exact_and_deterministic() {
        let text = "fn f(x: Int32) -> Bool { return x == 1; }";
        let (sources, _, output) = parse_text(text);
        assert_eq!(output.diagnostics(), None);
        let expected = "module @0..41\n  private function f @0..41\n    parameter x: Int32 @5..13\n    result Bool @15..22\n    block @23..41\n      return @25..39\n        compare eq @32..38\n          name x @32..33\n          integer 1 @37..38\n";
        assert_eq!(dump(output.module(), &sources), expected);
        assert_eq!(dump(output.module(), &sources), expected);
    }

    #[test]
    fn missing_closing_brace_drops_the_incomplete_function() {
        let (_, _, output) = parse_text("fn incomplete() { return;");
        assert!(output.diagnostics().is_some());
        assert!(output.module().functions.is_empty());
    }

    #[test]
    fn chained_comparison_has_one_specific_diagnostic() {
        let (_, _, output) =
            parse_text("fn bad(a: Int32, b: Int32, c: Int32) -> Bool { return a < b < c; }");
        assert!(codes(&output).contains(&CHAINED_COMPARISON));
        assert!(matches!(
            output.module().functions[0].body.statements[0],
            AstStatement::Error(_)
        ));
    }

    #[test]
    fn diagnostic_cap_adds_exactly_one_final_marker() {
        let limits = FrontendLimits::new(100, 32, 2).unwrap();
        let (_, _, output) =
            parse_text_with_limits("fn bad() { true; false; true; false; }", limits);
        assert_eq!(codes(&output).len(), 3);
        assert_eq!(codes(&output).last(), Some(&TRUNCATED));
        assert!(output.is_truncated());
    }

    #[test]
    fn syntax_depth_limit_is_exact_and_non_panicking() {
        let exact = FrontendLimits::new(100, 4, 10).unwrap();
        let (_, _, exact_output) =
            parse_text_with_limits("fn f() -> Bool { return (((true))); }", exact);
        assert_eq!(exact_output.diagnostics(), None);

        let limited = FrontendLimits::new(100, 3, 10).unwrap();
        let (_, _, limited_output) =
            parse_text_with_limits("fn f() -> Bool { return (((true))); }", limited);
        assert!(limited_output.is_truncated());
        assert_eq!(codes(&limited_output).last(), Some(&TRUNCATED));
    }

    #[test]
    fn postfix_spines_share_the_recursive_syntax_depth_budget() {
        let exact = FrontendLimits::new(100, 4, 10).unwrap();
        let (_, _, exact_output) = parse_text_with_limits("fn f() { target.member(); }", exact);
        assert_eq!(exact_output.diagnostics(), None);

        let mut source = String::from("fn f() { target");
        for _ in 0..1_024 {
            source.push_str(".member()");
        }
        source.push_str("; }");
        let limited = FrontendLimits::new(10_000, FrontendLimits::MAX_SYNTAX_DEPTH, 10).unwrap();
        let (_, _, output) = parse_text_with_limits(&source, limited);
        assert!(output.is_truncated());
        assert_eq!(codes(&output).last(), Some(&TRUNCATED));
    }

    #[test]
    fn parser_diagnostics_have_direct_source_origins() {
        let (sources, _, output) = parse_text("fn f() { true; }");
        let diagnostic = &output.diagnostics().unwrap().findings()[0];
        let span = sources.resolve_origin_span(diagnostic.origin()).unwrap();
        assert_eq!(spelling(&sources, span), "true");
    }

    #[test]
    fn repeated_parsing_is_structurally_deterministic() {
        let text = "fn f(x: Int32) -> Bool { if (x >= 1) { return true; } return false; }";
        let (_, _, first) = parse_text(text);
        let (_, _, second) = parse_text(text);
        assert_eq!(first, second);
    }

    #[test]
    fn parses_ps4_enum_and_both_switch_forms_with_exact_patterns() {
        let text = r"
            const State = enum { low, middle, high, };
            fn classify(value: Int32) -> State {
                const state: State = switch (value) {
                    -2147483648...-1 => .low,
                    0...20, 40 => State.middle,
                    else => .high,
                };
                switch (state) {
                    .low => { return .low; },
                    State.middle, .high => { return state; },
                }
            }
        ";
        let (sources, _, output) = parse_text(text);
        assert_eq!(output.diagnostics(), None);
        assert_eq!(output.module().enums.len(), 1);
        let dump = super::super::ast::dump(output.module(), &sources);
        assert!(dump.contains("enum State"));
        assert!(dump.contains("switch-expression"));
        assert!(dump.contains("-2147483648...-1"));
        assert!(dump.contains("State.middle"));
    }

    #[test]
    fn malformed_ps4_switch_recovers_before_the_next_function() {
        let (_, _, output) =
            parse_text("fn bad(x: Int32) { switch (x) { 0 1, else => {} } } fn good() {}");
        assert!(output.diagnostics().is_some());
        assert!(
            output
                .module()
                .functions
                .iter()
                .any(|function| function.body.statements.is_empty())
        );
    }
}
