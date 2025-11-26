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

fn test_next_token() {
	str := '_ a a10 AA = a+ =a [ a '
	mut l := qlex(str)
	mut count := 0
	kinds := [TokenKind.ident, .ident, .ident, .ident, .assign, .ident, .plus, .assign, .ident,
		.invalid, .ident]
	for t in l {
		assert t.kind == kinds[count], '${t} is not with ${kinds[count]}'
		count++
	}
}

fn test_parse_1() {
	str := 'a = b;'
	res := parse(str)
	expected := Expr(BinaryExpr{
		left:     Expr(Identifier{
			name: 'a'
		})
		operator: .assign
		right:    Expr(Identifier{
			name: 'b'
		})
	})
	assert res[0] == expected
}

fn test_parse_statement() {
	str := 'a = b; b = c;'
	res := parse(str)
	expected := [
		Expr(BinaryExpr{
			left:     Expr(Identifier{
				name: 'a'
			})
			operator: .assign
			right:    Expr(Identifier{
				name: 'b'
			})
		}),
		Expr(BinaryExpr{
			left:     Expr(Identifier{
				name: 'b'
			})
			operator: .assign
			right:    Expr(Identifier{
				name: 'c'
			})
		}),
	]
	assert res == expected
}

fn test_parse_statement2() {
	str := 'a = a == b;'
	res := parse(str)
	expected := [
		Expr(BinaryExpr{
			left:     Expr(Identifier{
				name: 'a'
			})
			operator: .assign
			right:    Expr(BinaryExpr{
				left:     Expr(Identifier{
					name: 'a'
				})
				operator: .eq
				right:    Expr(Identifier{
					name: 'b'
				})
			})
		}),
	]
	assert res == expected
}
