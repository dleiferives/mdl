module main

fn qlex(s string) Lexer {
	return Lexer{
		src:   s
		index: 0
	}
}

fn test_lexer_init() {
	a := Lexer{
		src: 'Hello'
	}
	assert a.src == 'Hello'
	assert a.index == 0
}

fn test_lexer_skip_whitespace() {
	mut a := Lexer{
		src: ' A'
	}
	a.skip_whitespace()
	assert a.index == 1
}

fn test_read_ident() {
	mut l := qlex('albright')
	t := l.read_identifier()
	assert t.kind == .ident
	assert t.str == 'albright'
	assert t.pos == 0
}

fn test_fail_ident() {
	mut l := qlex('1ahaba')
	t := l.read_identifier()
	assert t.kind == .invalid
	assert t.str == ''
	assert t.pos == -1
}

fn test_many_ident() {
	idents := ['a', 'A', '[', '_', '_A', '!', 'brighton', '____Balance', 'fors00th']
	mut count := 0
	for ident in idents {
		err_name := ident
		mut l := qlex(ident)
		t := l.read_identifier()
		match count {
			2, 5 {
				assert t.kind == .invalid, 'assertion failed for: ${ident} ${t}'
			}
			else {
				assert t.kind == .ident, 'assertion failed for: ${ident} ${t}'
				assert t.str == ident, 'assertion failed for: ${ident} ${t}'
				assert t.pos == 0, 'assertion failed for: ${ident} ${t}'
			}
		}
		count++
	}
}
