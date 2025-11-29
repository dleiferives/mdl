module main

enum TokenKind {
	invalid
	lit_string
	lit_integer
	lit_char
	eof
	ident
	define
	kw_if
	kw_else
	kw_fn
	kw_reg
	kw_data
	kw_eff
	kw_int
	kw_float
	kw_list
	kw_string
	kw_dict
	kw_void
	kw_namespace
	kw_return
	eq
	assign
	semicolon
	colon
	comma
	slash
	lcarrot
	rcarrot
	ampersand
	at
	dot
	plus
	minus
	star
	percent
	dotdot
	store
	lte
	gte
	ne
	plus_assign
	minus_assign
	star_assign
	slash_assign
	percent_assign
	swap
	lsquare
	rsquare
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
	return (c >= `a` && c <= `z`) || (c >= `A` && c <= `Z`) || c == `_` || c == `#`
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

pub fn (mut l Lexer) read_integer() Token {
	start := l.index
	iter: for (l.index < l.src.len) {
		r := l.src[l.index]
		match true {
			is_digit(r) {}
			else { break iter }
		}
		l.index++
	}
	str := l.src[start..l.index]

	return Token{
		kind: .lit_integer
		str:  str
		pos:  start
	}
}

pub fn (mut l Lexer) read_string() Token {
	start := l.index
	mut counter := 0
	iter: for (l.index < l.src.len) {
		r := l.src[l.index]
		match r {
			`\\` {
				l.index++
			}
			`\"` {
				if counter == 1 {
					l.index++
					break iter
				}
				counter++
			}
			else {}
		}
		l.index++
	}
	str := l.src[start..l.index]

	return Token{
		kind: .lit_string
		str:  str
		pos:  start
	}
}

pub fn (mut l Lexer) read_char() Token {
	start := l.index
	l.index++

	if l.index < l.src.len && l.src[l.index] == `\\` {
		l.index += 2
	} else if l.index < l.src.len {
		l.index++
	}

	if l.index < l.src.len && l.src[l.index] == `'` {
		l.index++
	}

	str := l.src[start..l.index]
	return Token{
		kind: .lit_char
		str:  str
		pos:  start
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
		'else' { TokenKind.kw_else }
		'fn' { TokenKind.kw_fn }
		'reg' { TokenKind.kw_reg }
		'data' { TokenKind.kw_data }
		'eff' { TokenKind.kw_eff }
		'Int' { TokenKind.kw_int }
		'Float' { TokenKind.kw_float }
		'List' { TokenKind.kw_list }
		'String' { TokenKind.kw_string }
		'Dict' { TokenKind.kw_dict }
		'Void' { TokenKind.kw_void }
		'namespace' { TokenKind.kw_namespace }
		'return' { TokenKind.kw_return }
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
		`/` {
			match pp {
				`=` {
					l.index += 2
					return Token{.slash_assign, '/=', l.index - 2}
				}
				else {
					l.index++
					return Token{.slash, '/', l.index - 1}
				}
			}
		}
		`+` {
			match pp {
				`=` {
					l.index += 2
					return Token{.plus_assign, '+=', l.index - 2}
				}
				else {
					l.index++
					return Token{.plus, '+', l.index - 1}
				}
			}
		}
		`-` {
			match pp {
				`=` {
					l.index += 2
					return Token{.minus_assign, '-=', l.index - 2}
				}
				else {
					l.index++
					return Token{.minus, '-', l.index - 1}
				}
			}
		}
		`*` {
			match pp {
				`=` {
					l.index += 2
					return Token{.star_assign, '*=', l.index - 2}
				}
				else {
					l.index++
					return Token{.star, '*', l.index - 1}
				}
			}
		}
		`%` {
			match pp {
				`=` {
					l.index += 2
					return Token{.percent_assign, '%=', l.index - 2}
				}
				else {
					l.index++
					return Token{.percent, '%', l.index - 1}
				}
			}
		}
		`@` {
			l.index++
			return Token{.at, '@', l.index - 1}
		}
		`{` {
			l.index++
			return Token{.lcurly, '{', l.index - 1}
		}
		`}` {
			l.index++
			return Token{.rcurly, '}', l.index - 1}
		}
		`(` {
			l.index++
			return Token{.lparen, '(', l.index - 1}
		}
		`)` {
			l.index++
			return Token{.rparen, ')', l.index - 1}
		}
		`[` {
			l.index++
			return Token{.lsquare, '[', l.index - 1}
		}
		`]` {
			l.index++
			return Token{.rsquare, ']', l.index - 1}
		}
		`,` {
			l.index++
			return Token{.comma, ',', l.index - 1}
		}
		`.` {
			match pp {
				`.` {
					l.index += 2
					return Token{.dotdot, '..', l.index - 2}
				}
				else {
					l.index++
					return Token{.dot, '.', l.index - 1}
				}
			}
		}
		`;` {
			l.index++
			return Token{.semicolon, ';', l.index - 1}
		}
		`<` {
			match pp {
				`=` {
					l.index += 2
					return Token{.lte, '<=', l.index - 2}
				}
				`-` {
					l.index += 2
					return Token{.store, '<-', l.index - 2}
				}
				else {
					l.index++
					return Token{.lcarrot, '<', l.index - 1}
				}
			}
		}
		`>` {
			match pp {
				`=` {
					l.index += 2
					return Token{.gte, '>=', l.index - 2}
				}
				`<` {
					l.index += 2
					return Token{.swap, '><', l.index - 2}
				}
				else {
					l.index++
					return Token{.rcarrot, '>', l.index - 1}
				}
			}
		}
		`&` {
			l.index++
			return Token{.ampersand, '&', l.index - 1}
		}
		`!` {
			match pp {
				`=` {
					l.index += 2
					return Token{.ne, '!=', l.index - 2}
				}
				else {
					l.index++
					return Token{.invalid, '!', l.index - 1}
				}
			}
		}
		`\"` {
			return l.read_string()
		}
		`'` {
			return l.read_char()
		}
		else {}
	}
	match true {
		is_alpha(p) {
			return l.read_identifier()
		}
		is_digit(p) {
			return l.read_integer()
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
