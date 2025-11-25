module main

struct Pratt {
mut:
	id int
}

enum TokenKind {
	invalid
	eof
	ident
	eq
	plus
	lparen
	rparen
}

enum Precidence {
	lowest
	assign
	sum
	prefix
}

struct Token {
	kind TokenKind
	str  string
	pos  int
}

// LEXER
pub struct Lexer {
	src string
mut:
	index int // zero by default...
}

pub fn is_alpha(c rune) bool {
	return (c >= `a` && c <= `z`) || (c >= `A` && c <= `Z`) || c == `_`
}

pub fn is_digit(c rune) bool {
	return c >= `0` && c <= `9`
}

pub fn (mut l Lexer) skip_whitespace() {
	for (l.index < l.src.len && match l.src[l.index] {
		` ` { true }
		`\n` { true }
		`\t` { true }
		`\r` { true }
		else { false }
	}) {
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
	return Token{
		kind: TokenKind.ident
		str:  l.src[start..l.index]
		pos:  start
	}
}

// AST
type Expr = BinaryExpr | Identifier

struct BinaryExpr {
	left     Expr
	operator TokenKind
	right    Expr
}

struct Identifier {
	name string
}
