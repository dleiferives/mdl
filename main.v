module main

struct Pratt {
mut:
	id int
}

enum TokenKind {
	invalid
	eof
	ident
	define
	kw_if
	kw_fn
	kw_reg
	kw_data
	kw_eff
	kw_int
	kw_float
	kw_list
	kw_string
	kw_dict
	kw_namespace
	// TODO: add the rest of the binary operations that we will support
	eq
	assign
	semicolon
	colon
	plus
	lparen
	rparen
	lcurly
	rcurly
}

struct Token {
	kind TokenKind
	str  string
	pos  int
}

pub fn is_alpha(c rune) bool {
	return (c >= `a` && c <= `z`) || (c >= `A` && c <= `Z`) || c == `_`
}

pub fn is_digit(c rune) bool {
	return c >= `0` && c <= `9`
}

// LEXER
// or tokenizer... whatever you want to call it
// turns strings into tokens
pub struct Lexer {
	src string
mut:
	index int // zero by default...
}

pub fn (mut l Lexer) skip_to_whitespace() {
	for (l.index < l.src.len) {
		cond := match l.src[l.index] {
			` ` { true }
			`\n` { true }
			`\t` { true }
			`\r` { true }
			else { false }
		}

		if cond {
			return
		}

		l.index++
	}
}

pub fn (mut l Lexer) skip_whitespace() {
	for (l.index < l.src.len) {
		cond := match l.src[l.index] {
			` ` { true }
			`\n` { true }
			`\t` { true }
			`\r` { true }
			else { false }
		}

		if !cond {
			return
		}

		l.index++
	}
}

pub fn (mut l Lexer) read_identifier() Token {
	start := l.index
	mut count := 0
	iter: for (l.index < l.src.len) {
		r := l.src[l.index]
		match true {
			is_alpha(r) {}
			is_digit(r) && count > 0 {}
			else { break iter }
		}
		l.index++
		count++
	}
	if start == l.index {
		return Token{
			kind: TokenKind.invalid
			str:  ''
			pos:  -1
		}
	}

	// handle keywords here...

	str := l.src[start..l.index]
	kind := match str {
		'if' { TokenKind.kw_if }
		'fn' { TokenKind.kw_fn }
		'reg' { TokenKind.kw_reg }
		'data' { TokenKind.kw_data }
		'eff' { TokenKind.kw_eff }
		'Int' { TokenKind.kw_int }
		'Float' { TokenKind.kw_float }
		'List' { TokenKind.kw_list }
		'String' { TokenKind.kw_string }
		'Dict' { TokenKind.kw_dict }
		'namespace' { TokenKind.kw_namespace }
		else { TokenKind.ident }
	}

	return Token{
		kind: kind
		str:  str
		pos:  start
	}
}

pub fn (mut l Lexer) peekn(n int) rune {
	if l.index + n >= l.src.len {
		return rune(0)
	}
	return l.src.runes()[l.index + n]
}

pub fn (mut l Lexer) peek() rune {
	if l.index >= l.src.len {
		return rune(0)
	}
	return l.src.runes()[l.index]
}

pub fn (mut l Lexer) advance() rune {
	if l.index >= l.src.len {
		return rune(0)
	}
	result := l.src.runes()[l.index]
	l.index++
	return result
}

pub fn (mut l Lexer) next_token() Token {
	l.skip_whitespace()
	p := l.peek()
	pp := l.peekn(1)
	if p == rune(0) {
		return Token{
			kind: .eof
			str:  ''
			pos:  l.index
		}
	}
	match p {
		`=` {
			match pp {
				`=` {
					l.index += 2
					return Token{.eq, '==', l.index - 2}
				}
				else {
					l.index++
					return Token{.assign, '=', l.index - 1}
				}
			}
		}
		`:` {
			match pp {
				`=` {
					l.index += 2
					return Token{.define, ':=', l.index - 2}
				}
				else {
					l.index++
					return Token{.colon, ':', l.index - 1}
				}
			}
		}
		`+` {
			l.index++
			return Token{.plus, '+', l.index - 1}
		}
		`{` {
			l.index++
			return Token{.lcurly, '{', l.index - 1}
		}
		`}` {
			l.index++
			return Token{.rcurly, '}', l.index - 1}
		}
		`;` {
			l.index++
			return Token{.semicolon, ';', l.index - 1}
		}
		else {}
	}
	match true {
		is_alpha(p) {
			return l.read_identifier()
		}
		else {
			start := l.index
			l.skip_to_whitespace()
			return Token{
				kind: .invalid
				str:  l.src[start..l.index]
				pos:  start
			}
		}
	}
}

pub fn (mut l Lexer) next() ?Token {
	t := l.next_token()
	if t.kind == .eof {
		return none
	}
	return t
}

// PARSER -> Turns into the ast
struct Parser {
mut:
	lexer   Lexer
	current Token
	next    Token
}

type Precidence = u8

// TODO: should move
fn (t TokenKind) precidence() Precidence {
	return match t {
		.eq { 2 }
		else { 0 }
	}
}

// AST
type Expr = BinaryExpr | Identifier | Invalid | Define

enum ValueType {
	effemeral
	register
	data
}

struct BinaryExpr {
	left     Expr
	operator TokenKind
	right    Expr
}

struct Identifier {
	name string
}

struct Define {
	source ValueType
	name   Identifier
	value  Expr
}

struct Invalid {}

fn (mut p Parser) consume(kind TokenKind) bool {
	if p.current.kind == kind {
		p.advance()
		return true
	}
	print('was expecting ${kind} found  ${p.current.kind} instead at ${p.current}')
	return false
}

fn (mut p Parser) advance() {
	p.current = p.next
	p.next = p.lexer.next_token()
}

fn (mut p Parser) parse_ident() Identifier {
	cur := p.current
	if p.current.kind != .ident {
		panic('was expecting ident...')
	}
	p.advance()
	return Identifier{
		name: cur.str
	}
}

fn (mut p Parser) parse_definition() Expr {
	source := match p.current.kind {
		.kw_reg {
			ValueType.register
		}
		.kw_data {
			ValueType.data
		}
		.kw_eff {
			ValueType.effemeral
		}
		else {
			panic('illegal state')
			ValueType.register
		}
	}
	p.advance()
	if p.current.kind != .ident {
		panic('should be an ident.... check grammer')
	}
	name := p.parse_ident()

	if p.current.kind == .define {
		p.advance()
		value := p.parse_expr(0)
		return Expr(Define{source, name, value})
	}
	panic('was expecting a define??')
}

fn (mut p Parser) parse_prefix() Expr {
	match p.current.kind {
		.ident {
			return Expr(p.parse_ident())
		}
		.kw_reg, .kw_data, .kw_eff {
			return p.parse_definition()
		}
		else {
			print('\n\nillegal state hit... ${p}\n')
			p.advance()
			return Expr(Invalid{})
		}
	}
}

fn (mut p Parser) parse_expr(min_prec Precidence) Expr {
	mut left := p.parse_prefix()

	for {
		prec := p.current.kind.precidence()
		if prec <= min_prec {
			break
		}
		op := p.current.kind
		p.advance()

		// TODO: handle other comparison operators
		if op == .eq {
			right := p.parse_expr(prec + 1)
			left = Expr(BinaryExpr{left, op, right})
			continue
		}
	}
	return left
}

fn (mut p Parser) parse_statement() Expr {
	left := p.parse_expr(0)

	// If we have some form of operator on our statement?
	// TODO: handle other things as I add them
	if p.current.kind == .assign {
		op := p.current.kind
		p.advance()
		right := p.parse_expr(0)
		p.consume(.semicolon)
		return Expr(BinaryExpr{
			left:     left
			operator: op
			right:    right
		})
	}

	p.consume(.semicolon)
	return left
}

fn parse(str string) []Expr {
	mut p := Parser{
		lexer: Lexer{
			src:   str
			index: 0
		}
	}
	p.current = p.lexer.next_token()
	p.next = p.lexer.next_token()

	mut statements := []Expr{}
	for p.current.kind != .eof {
		stmt := p.parse_statement()
		statements << stmt
	}
	return statements
}
