module main

struct Pratt {
mut:
	id int
}

enum TokenKind {
	invalid
	eof
	ident
	// TODO: add the rest of the binary operations that we will support
	eq
	assign
	semicolon
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
	return Token{
		kind: TokenKind.ident
		str:  l.src[start..l.index]
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
		`+` {
			l.index++
			return Token{.plus, '+', l.index - 1}
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
