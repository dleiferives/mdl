struct Pratt {
mut:
	id int
}

enum TokenKind {
	EOF
	IDENT
	EQ
	PLUS
	LPAREN
	RPAREN
}

struct Token {
	kind TokenKind
	str  string
	pos  int
}

struct Lexer {
	src string
mut:
	index int
}

fn is_alpha(c rune) bool {
	return (c >= `a` && c <= `z`) || (c >= `A` && c <= `Z`) || c == `_`
}

fn is_digit(c rune) bool {
	return c >= `0` && c <= `9`
}

mut test := 'a = 22'
mut thing := match true {
	is_alpha(test[0]) { 'ALPHA' }
	else { 'else' }
}

println(test)
print(thing)
