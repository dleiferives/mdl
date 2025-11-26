module main

// TODO: inline functions... later problem lol

struct Pratt {
mut:
	id int
}

enum TokenKind {
	invalid
	lit_string
	lit_integer
	lit_char
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
	kw_void
	kw_namespace
	kw_return
	// TODO: add the rest of the binary operations that we will support
	eq
	assign
	semicolon
	colon
	comma
	slash
	lcarrot
	rcarrot
	ampersand
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
		.assign, .plus_assign, .minus_assign, .star_assign, .slash_assign, .percent_assign, .store { 1 }
		.eq, .ne, .lte, .gte, .lcarrot, .rcarrot, .swap { 2 }
		.plus, .minus { 3 }
		.star, .slash, .percent { 4 }
		else { 0 }
	}
}

// AST
type Expr = BinaryExpr
	| Identifier
	| Invalid
	| Define
	| Dictionary
	| Macro
	| Integer
	| String
	| Char
	| List
	| Range
	| Store
	| IdentifierChain
	| FunctionDefinition
	| FunctionInlineDefinition
	| IfStatement
	| FunctionCall
	| StringSlice
	| IndexExpr
	| NamespaceDefinition
	| NamespaceImport
	| NamespaceAlias
	| ReturnStatement
	| InterpolatedString
	| QualifiedIdentifier
	| ReferenceExpr

struct IfStatement {
	condition Expr
	block     Block
}

type FunctionChainElement = Identifier | Macro

struct FunctionCall {
	namespace_path []Identifier
	name_chain     []FunctionChainElement
	args           []Expr
}

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

enum IdentifierChainKind {
	identchk_ident
	identchk_macro
	identchk_array_integer
	identchk_integer
	identchk_array_macro
}

struct IdentifierChain {
mut:
	kind          IdentifierChainKind
	ident         ?Identifier
	array_integer ?int
	integer       ?int
	macro         ?&Macro
	next          ?&IdentifierChain
}

struct Macro {
mut:
	referable   bool
	ident_chain IdentifierChain
}

struct Integer {
	value int
}

struct String {
	value string
}

struct Char {
	value string
}

struct List {
	entries []Expr
}

struct Range {
	start ?Expr
	end   ?Expr
}

struct Store {
	left  Expr
	right Expr
}

struct StringSlice {
	target Expr
	start  ?Expr
	end    ?Expr
}

struct IndexExpr {
	target Expr
	index  Expr
}

enum DictionaryEntryKind {
	dictentk_integer
	dictentk_string
	dictentk_macro
}

struct DictionaryEntry {
mut:
	kind    DictionaryEntryKind
	integer ?int
	str     ?String
	macro   ?Macro
	value   Expr
}

struct Dictionary {
	entries []DictionaryEntry
}

struct Block {
	exprs []Expr
}

// todo add user created... though those would be dicts so like.. allow renaming?
enum Type {
	int
	float
	list
	string
	dict
	void
}
type ReferenceInternal = Type | ReferenceType

struct ReferenceType {
	base ReferenceInternal
}

struct FunctionArgument {
	source   ValueType
	name     Identifier
	arg_type ReferenceInternal
}

struct FunctionDefinition {
	ident       Identifier
	args        []FunctionArgument
	return_type ReferenceInternal
	block       Block
}

struct FunctionInlineArgument {
	source   ValueType
	name     Identifier
	arg_type ReferenceInternal
	op       TokenKind
	value    Expr
}

struct FunctionInlineDefinition {
	ident Identifier
	args  []FunctionInlineArgument
	block Block
}

struct NamespaceDefinition {
	name  Identifier
	block Block
}

struct NamespaceImport {
	name Identifier
	path String
}

struct NamespaceAlias {
	name Identifier
}

struct ReturnStatement {
	value ?Expr
}

struct InterpolatedStringPart {
	is_macro bool
	text     string
	macro    ?Macro
}

struct InterpolatedString {
	parts []InterpolatedStringPart
}

struct QualifiedIdentifier {
	namespace_path []Identifier
	name           Identifier
}

struct ReferenceExpr {
	target Expr
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

fn (mut p Parser) consume_silent(kind TokenKind) bool {
	if p.current.kind == kind {
		p.advance()
		return true
	}
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

fn (mut p Parser) parse_type() ReferenceInternal {
	if p.current.kind == .ampersand {
		p.consume(.ampersand) // consume the '&'
		base_type := p.parse_type() // recursively parse the underlying type
		return ReferenceType{
			base: base_type
		}
	}
	match p.current.kind {
		.kw_int {
			p.advance()
			return Type.int
		}
		.kw_float {
			p.advance()
			return Type.float
		}
		.kw_list {
			p.advance()
			return Type.list
		}
		.kw_string {
			p.advance()
			return Type.string
		}
		.kw_dict {
			p.advance()
			return Type.dict
		}
		.kw_void {
			p.advance()
			return Type.void
		}
		else {
			panic('expected type keyword (Int, Float, List, String, Dict, Void) got ${p.current}')
		}
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

fn (mut p Parser) parse_arrayaccess() ?&IdentifierChain {
	if p.current.kind == .lsquare {
		mut result := &IdentifierChain{}
		p.consume(.lsquare)
		match p.current.kind {
			.lit_integer {
				result.kind = .identchk_array_integer
				result.array_integer = p.current.str.int()
				p.advance()
			}
			.lcarrot {
				result.kind = .identchk_array_macro
				result.macro = p.parse_macro()
			}
			else {
				panic('impossible state in parsing array access')
			}
		}

		p.consume(.rsquare)
		next := p.parse_arrayaccess()
		result.next = next
		return result
	}
	return none
}

fn (mut p Parser) parse_identchainrec() ?&IdentifierChain {
	if p.current.kind != .dot {
		return none
	}
	p.consume(.dot)

	first := p.current
	mut result := &IdentifierChain{}
	mut resultptr := result
	match first.kind {
		.ident {
			result.kind = .identchk_ident
			result.ident = p.parse_ident()
			result.next = p.parse_arrayaccess()
			for result.next != none {
				result = result.next or { panic('impossible') }
			}
			result.next = p.parse_identchainrec()
		}
		.lcarrot {
			result.kind = .identchk_macro
			result.macro = p.parse_macro()
			result.next = p.parse_arrayaccess()
			for result.next != none {
				result = result.next or { panic('impossible') }
			}
			result.next = p.parse_identchainrec()
		}
		.lit_integer {
			result.kind = .identchk_integer
			result.integer = p.current.str.int()
			p.advance()
			result.next = p.parse_arrayaccess()
			for result.next != none {
				result = result.next or { panic('impossible') }
			}
			result.next = p.parse_identchainrec()
		}
		else {}
	}
	return resultptr
}

fn (mut p Parser) parse_identchainstart() IdentifierChain {
	first := p.current
	mut resultt := IdentifierChain{}
	mut result := &resultt
	match first.kind {
		.ident {
			result.kind = .identchk_ident
			result.ident = p.parse_ident()
			result.next = p.parse_arrayaccess()
			for result.next != none {
				result = result.next or { panic('impossible') }
			}
			result.next = p.parse_identchainrec()
		}
		.lcarrot {
			result.kind = .identchk_macro
			result.macro = p.parse_macro()
			result.next = p.parse_arrayaccess()
			for result.next != none {
				result = result.next or { panic('impossible') }
			}
			result.next = p.parse_identchainrec()
		}
		else {}
	}
	return resultt
}

fn (mut p Parser) parse_macro() &Macro {
	mut result := &Macro{}
	p.consume(.lcarrot)
	result.referable = p.consume_silent(.ampersand)
	result.ident_chain = p.parse_identchainstart()

	p.consume(.rcarrot)
	return result
}

fn (mut p Parser) parse_dictionary_entry() DictionaryEntry {
	// Some Name

	mut result := DictionaryEntry{}
	match p.current.kind {
		.lit_string {
			result.kind = DictionaryEntryKind.dictentk_string
			result.str = String{p.current.str}
			p.advance()
		}
		.lit_integer {
			result.kind = DictionaryEntryKind.dictentk_integer
			result.integer = p.current.str.int()
			p.advance()
		}
		// in this case it must be a macro otherwise the code is poorly formed
		.lcarrot {
			result.macro = p.parse_macro()
		}
		else {
			panic('illegal state when parsing dictionary entry')
		}
	}
	// COLON
	p.consume(.colon)
	// EXPR
	result.value = p.parse_expr(0)
	return result
}

fn (mut p Parser) parse_dictionary() Dictionary {
	p.consume(.lcurly)
	if p.next.kind == .rcurly {
		return Dictionary{}
	}
	mut result := []DictionaryEntry{}
	for p.current.kind != .rcurly {
		if p.current.kind == .comma {
			panic('illegal state dictionary improperly setup')
		}
		result << p.parse_dictionary_entry()
		if p.current.kind == .comma {
			p.consume(.comma)
		}
	}
	p.consume(.rcurly)
	return Dictionary{result}
}

fn (mut p Parser) parse_list() List {
	p.consume(.lsquare)
	mut entries := []Expr{}

	if p.current.kind != .rsquare {
		entries << p.parse_expr(0)

		for p.current.kind == .comma {
			p.consume(.comma)
			if p.current.kind == .rsquare {
				break
			}
			entries << p.parse_expr(0)
		}
	}

	p.consume(.rsquare)
	return List{entries}
}

fn (mut p Parser) parse_range(left ?Expr) Range {
	p.consume(.dotdot)

	mut end := ?Expr(none)
	if p.current.kind != .rparen && p.current.kind != .semicolon {
		end = p.parse_expr(0)
	}

	return Range{
		start: left
		end:   end
	}
}

fn (mut p Parser) parse_bracket_postfix(target Expr) Expr {
	p.consume(.lsquare)

	if p.current.kind == .colon {
		mut end_expr := ?Expr(none)
		p.consume(.colon)
		if p.current.kind != .rsquare {
			end_expr = p.parse_expr(0)
		}
		p.consume(.rsquare)
		return Expr(StringSlice{
			target: target
			start:  none
			end:    end_expr
		})
	}

	if p.current.kind == .rsquare {
		p.consume(.rsquare)
		return Expr(StringSlice{
			target: target
			start:  none
			end:    none
		})
	}

	first_expr := p.parse_expr(0)

	if p.current.kind == .colon {
		p.consume(.colon)
		mut end_expr := ?Expr(none)
		if p.current.kind != .rsquare {
			end_expr = p.parse_expr(0)
		}
		p.consume(.rsquare)
		return Expr(StringSlice{
			target: target
			start:  first_expr
			end:    end_expr
		})
	} else {
		p.consume(.rsquare)
		return Expr(IndexExpr{
			target: target
			index:  first_expr
		})
	}
}

fn (mut p Parser) parse_function_argument() FunctionArgument {
	mut source := ValueType.effemeral
	match p.current.kind {
		.kw_reg { source = .register }
		.kw_data { source = .data }
		.kw_eff { source = .effemeral }
		else { panic('expected argument source: reg | data | eff') }
	}
	p.advance()

	if p.current.kind != .ident {
		panic('expected identifier for function argument name')
	}
	name := p.parse_ident()
	p.consume(.colon)
	// type
	arg_type := p.parse_type()

	return FunctionArgument{
		source:   source
		name:     name
		arg_type: arg_type
	}
}

fn (mut p Parser) parse_function_arguments() []FunctionArgument {
	mut args := []FunctionArgument{}
	p.consume(.lparen)

	if p.current.kind != .rparen {
		args << p.parse_function_argument()
		for p.current.kind == .comma {
			p.consume(.comma)
			args << p.parse_function_argument()
		}
	}

	p.consume(.rparen)
	return args
}

fn (mut p Parser) parse_function_definition() Expr {
	p.consume(.kw_fn)

	if p.current.kind != .ident {
		panic('expected function identifier after "fn"')
	}
	ident := p.parse_ident()

	args := p.parse_function_arguments()

	p.consume(.colon)

	ret_ty := p.parse_type()

	block := p.parse_block()

	return Expr(FunctionDefinition{
		ident:       ident
		args:        args
		return_type: ret_ty
		block:       block
	})
}

fn (mut p Parser) parse_function_inline_argument() FunctionInlineArgument {
	mut source := ValueType.effemeral
	match p.current.kind {
		.kw_reg { source = .register }
		.kw_data { source = .data }
		.kw_eff { source = .effemeral }
		else { panic('expected argument source: reg | data | eff') }
	}
	p.advance()

	if p.current.kind != .ident {
		panic('expected identifier for function argument name')
	}
	name := p.parse_ident()
	p.consume(.colon)
	arg_type := p.parse_type()

	op := p.current.kind
	match op {
		.assign, .store {
			p.advance()
		}
		else {
			panic('expected "=" or "<-" for inline function argument assignment')
		}
	}

	value := p.parse_expr(0)

	return FunctionInlineArgument{
		source:   source
		name:     name
		arg_type: arg_type
		op:       op
		value:    value
	}
}

fn (mut p Parser) parse_function_inline_arguments() []FunctionInlineArgument {
	mut args := []FunctionInlineArgument{}
	p.consume(.lcarrot)

	if p.current.kind != .rcarrot {
		args << p.parse_function_inline_argument()
		for p.current.kind == .comma {
			p.consume(.comma)
			args << p.parse_function_inline_argument()
		}
	}

	p.consume(.rcarrot)
	return args
}

fn (mut p Parser) parse_function_inline_definition() Expr {
	if p.current.kind != .ident {
		panic('expected function identifier for inline function definition')
	}
	ident := p.parse_ident()

	args := p.parse_function_inline_arguments()

	block := p.parse_block()

	return Expr(FunctionInlineDefinition{
		ident: ident
		args:  args
		block: block
	})
}

fn (mut p Parser) parse_if_statement() Expr {
	p.consume(.kw_if)
	p.consume(.lparen)

	condition := p.parse_expr(0)

	p.consume(.rparen)

	block := p.parse_block()

	return Expr(IfStatement{
		condition: condition
		block:     block
	})
}

fn (mut p Parser) parse_function_identifier_chain() ([]Identifier, []FunctionChainElement) {
	mut namespace_path := []Identifier{}
	mut chain := []FunctionChainElement{}

	if p.current.kind == .ident {
		saved_index := p.lexer.index
		saved_current := p.current
		saved_next := p.next

		mut temp_path := []Identifier{}
		temp_path << p.parse_ident()

		for p.current.kind == .dot {
			p.consume(.dot)
			if p.current.kind == .ident {
				temp_path << p.parse_ident()
			} else {
				break
			}
		}

		// If we have a slash after the path, it's a namespace path
		if p.current.kind == .slash && temp_path.len > 1 {
			namespace_path = temp_path.clone()
			chain << FunctionChainElement(temp_path.last())
		} else {
			// Restore and parse normally
			p.lexer.index = saved_index
			p.current = saved_current
			p.next = saved_next

			match p.current.kind {
				.ident {
					chain << FunctionChainElement(p.parse_ident())
				}
				.lcarrot {
					chain << FunctionChainElement(*p.parse_macro())
				}
				else {
					panic('expected identifier or macro at start of function chain')
				}
			}
		}
	} else if p.current.kind == .lcarrot {
		chain << FunctionChainElement(*p.parse_macro())
	}

	for p.current.kind == .slash {
		p.consume(.slash)

		match p.current.kind {
			.ident {
				chain << FunctionChainElement(p.parse_ident())
			}
			.lcarrot {
				chain << FunctionChainElement(*p.parse_macro())
			}
			else {
				panic('expected identifier or macro after "/" in function chain')
			}
		}
	}

	return namespace_path, chain
}

fn (mut p Parser) parse_function_call(namespace_path []Identifier, name_chain []FunctionChainElement) Expr {
	mut args := []Expr{}

	p.consume(.lparen)

	if p.current.kind != .rparen {
		if p.current.kind == .ampersand {
			args << p.parse_reference_expr()
		} else {
			args << p.parse_expr(0)
		}

		for p.current.kind == .comma {
			p.consume(.comma)
			if p.current.kind == .ampersand {
				args << p.parse_reference_expr()
			} else {
				args << p.parse_expr(0)
			}
		}
	}

	p.consume(.rparen)

	return Expr(FunctionCall{
		namespace_path: namespace_path
		name_chain:     name_chain
		args:           args
	})
}

fn (mut p Parser) parse_namespace_definition() Expr {
	p.consume(.kw_namespace)

	if p.current.kind != .ident {
		panic('expected identifier after namespace')
	}
	name := p.parse_ident()

	p.consume(.assign)

	block := p.parse_block()

	return Expr(NamespaceDefinition{
		name:  name
		block: block
	})
}

fn (mut p Parser) parse_namespace_import() Expr {
	p.consume(.kw_namespace)

	if p.current.kind != .ident {
		panic('expected identifier after namespace')
	}
	name := p.parse_ident()

	p.consume(.assign)

	if p.current.kind != .lit_string {
		panic('expected string literal for namespace import path')
	}
	path := String{
		value: p.current.str
	}
	p.advance()

	return Expr(NamespaceImport{
		name: name
		path: path
	})
}

fn (mut p Parser) parse_namespace_alias() Expr {
	p.consume(.kw_namespace)

	if p.current.kind != .ident {
		panic('expected identifier after namespace')
	}
	name := p.parse_ident()

	return Expr(NamespaceAlias{
		name: name
	})
}

fn (mut p Parser) parse_return_statement() Expr {
	p.consume(.kw_return)

	if p.current.kind == .semicolon {
		return Expr(ReturnStatement{
			value: none
		})
	}

	value := p.parse_expr(0)

	return Expr(ReturnStatement{
		value: value
	})
}

fn (mut p Parser) parse_interpolated_string(str_value string) Expr {
	mut parts := []InterpolatedStringPart{}
	mut current_text := ''
	mut i := 1

	for i < str_value.len - 1 {
		if str_value[i] == `<` {
			if current_text.len > 0 {
				parts << InterpolatedStringPart{
					is_macro: false
					text:     current_text
					macro:    none
				}
				current_text = ''
			}

			mut macro_start := i
			mut macro_end := i + 1
			for macro_end < str_value.len - 1 && str_value[macro_end] != `>` {
				macro_end++
			}

			if macro_end < str_value.len - 1 {
				macro_str := str_value[macro_start..macro_end + 1]
				// Parse the macro by creating a mini lexer
				mut macro_lexer := Lexer{
					src:   macro_str
					index: 0
				}
				mut macro_parser := Parser{
					lexer: macro_lexer
				}
				macro_parser.current = macro_parser.lexer.next_token()
				macro_parser.next = macro_parser.lexer.next_token()

				parsed_macro := macro_parser.parse_macro()

				parts << InterpolatedStringPart{
					is_macro: true
					text:     ''
					macro:    *parsed_macro
				}

				i = macro_end + 1
				continue
			}
		}

		current_text += str_value[i].ascii_str()
		i++
	}

	if current_text.len > 0 {
		parts << InterpolatedStringPart{
			is_macro: false
			text:     current_text
			macro:    none
		}
	}

	return Expr(InterpolatedString{
		parts: parts
	})
}

fn (mut p Parser) parse_qualified_identifier() Expr {
	mut namespace_path := []Identifier{}

	namespace_path << p.parse_ident()

	for p.current.kind == .dot {
		p.consume(.dot)
		if p.current.kind == .ident {
			namespace_path << p.parse_ident()
		} else {
			break
		}
	}

	name := namespace_path.pop()

	if namespace_path.len > 0 {
		return Expr(QualifiedIdentifier{
			namespace_path: namespace_path
			name:           name
		})
	}

	return Expr(name)
}

fn (mut p Parser) parse_reference_expr() Expr {
	p.consume(.ampersand)
	target := p.parse_expr(5)
	return Expr(ReferenceExpr{
		target: target
	})
}

fn (mut p Parser) parse_prefix() Expr {
	match p.current.kind {
		.kw_namespace {
			if p.next.kind == .ident {
				saved_index := p.lexer.index
				saved_current := p.current
				saved_next := p.next

				p.advance()
				p.advance()

				if p.current.kind == .assign {
					p.lexer.index = saved_index
					p.current = saved_current
					p.next = saved_next

					if p.next.kind == .ident {
						p.advance()
						p.advance()
						p.advance() // now at the value
						if p.current.kind == .lit_string {
							p.lexer.index = saved_index
							p.current = saved_current
							p.next = saved_next
							return p.parse_namespace_import()
						} else if p.current.kind == .lcurly {
							p.lexer.index = saved_index
							p.current = saved_current
							p.next = saved_next
							return p.parse_namespace_definition()
						}
					}
				} else if p.current.kind == .semicolon {
					p.lexer.index = saved_index
					p.current = saved_current
					p.next = saved_next
					return p.parse_namespace_alias()
				}
			}
			panic('illegal state in parsing namespace prefix')
		}
		.kw_return {
			return p.parse_return_statement()
		}
		.ampersand {
			return p.parse_reference_expr()
		}
		.ident, .lcarrot {
			saved_index := p.lexer.index
			saved_current := p.current
			saved_next := p.next

			namespace_path, name_chain := p.parse_function_identifier_chain()

			if p.current.kind == .lparen {
				return p.parse_function_call(namespace_path, name_chain)
			}

			p.lexer.index = saved_index
			p.current = saved_current
			p.next = saved_next

			if p.current.kind == .ident {
				saved2_index := p.lexer.index
				saved2_current := p.current
				saved2_next := p.next

				mut temp_idents := []Identifier{}
				temp_idents << p.parse_ident()

				for p.current.kind == .dot && p.next.kind == .ident {
					p.consume(.dot)
					temp_idents << p.parse_ident()
				}

				if temp_idents.len > 1 && p.current.kind != .lparen {
					name := temp_idents.pop()
					return Expr(QualifiedIdentifier{
						namespace_path: temp_idents
						name:           name
					})
				}

				p.lexer.index = saved2_index
				p.current = saved2_current
				p.next = saved2_next
			}

			match p.current.kind {
				.ident {
					return Expr(p.parse_ident())
				}
				.lcarrot {
					return Expr(*p.parse_macro())
				}
				else {
					panic('unexpected token in parse_prefix for ident/lcarrot') // Should ideally be caught earlier
				}
			}
		}
		.kw_reg, .kw_data, .kw_eff {
			if p.next.kind == .ident && p.lexer.peekn(2) == `<` {
				return p.parse_function_inline_definition()
			}
			return p.parse_definition()
		}
		.kw_fn {
			return p.parse_function_definition()
		}
		.lcurly {
			return Expr(p.parse_dictionary())
		}
		.lsquare {
			return Expr(p.parse_list())
		}
		.dotdot {
			return Expr(p.parse_range(none))
		}
		.lit_integer {
			val := p.current.str.int()
			p.advance()
			return Expr(Integer{
				value: val
			})
		}
		.lit_string {
			val := p.current.str
			p.advance()
			if val.contains('<') && val.contains('>') {
				return p.parse_interpolated_string(val)
			}
			return Expr(String{
				value: val
			})
		}
		.lit_char {
			val := p.current.str
			p.advance()
			return Expr(Char{
				value: val
			})
		}
		else {
			print('\n\nillegal state hit... ${p.current}\n')
			p.advance()
			return Expr(Invalid{})
		}
	}
	panic('Unhandled case in parse_prefix - reached end of function without return')
}

fn (mut p Parser) parse_postfix(mut left Expr) Expr {
	for {
		match p.current.kind {
			.lsquare {
				left = p.parse_bracket_postfix(left)
			}
			else {
				return left
			}
		}
	}
	return left
}

fn (mut p Parser) parse_expr(min_prec Precidence) Expr {
	mut left := p.parse_prefix()

	left = p.parse_postfix(mut left)

	for {
		if p.current.kind == .dotdot {
			left = Expr(p.parse_range(left))
			continue
		}

		prec := p.current.kind.precidence()
		if prec <= min_prec {
			break
		}
		op := p.current.kind
		p.advance()

		right := p.parse_expr(prec + 1)
		left = Expr(BinaryExpr{left, op, right})
	}
	return left
}

fn (mut p Parser) parse_statement() Expr {
	if p.current.kind == .kw_return {
		ret := p.parse_return_statement()
		p.consume(.semicolon)
		return ret
	}

	if p.current.kind == .kw_namespace {
		ns := p.parse_prefix()
		p.consume(.semicolon)
		return ns
	}

	left := p.parse_expr(0)

	// If we have some form of operator on our statement?
	// TODO: handle other things as I add them
	match p.current.kind {
		.assign, .plus_assign, .minus_assign, .star_assign, .slash_assign, .percent_assign, .eq,
		.ne, .lte, .gte, .lcarrot, .rcarrot, .swap, .plus, .minus, .star, .slash, .percent {
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
		.store {
			p.advance()
			right := p.parse_expr(0)
			p.consume(.semicolon)
			return Expr(Store{
				left:  left
				right: right
			})
		}
		else {}
	}

	p.consume(.semicolon)
	return left
}

fn (mut p Parser) parse_block() Block {
	p.consume(.lcurly)
	mut exprs := []Expr{}
	for p.current.kind != .rcurly && p.current.kind != .eof {
		exprs << p.parse_statement()
	}
	p.consume(.rcurly)
	return Block{
		exprs: exprs
	}
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

fn unparse_type(t ReferenceInternal) string {
	match t {
		Type {
			return match t {
				.int { 'Int' }
				.float { 'Float' }
				.list { 'List' }
				.string { 'String' }
				.dict { 'Dict' }
				.void { 'Void' }
			}
		}
		ReferenceType {
			return '&' + unparse_type(t.base)
		}
	}
}

fn unparse_one(ex Expr) string {
	mut result := ''
	match ex {
		IfStatement {
			result += 'if ('
			result += unparse_one(ex.condition)
			result += ') {\n'
			for expr in ex.block.exprs {
				result += '  ' + unparse_one(expr) + ';\n'
			}
			result += '}'
			return result
		}
		FunctionCall {
			for i, ns in ex.namespace_path {
				if i > 0 {
					result += '.'
				}
				result += ns.name
			}
			if ex.namespace_path.len > 0 {
				result += '/'
			}

			for i, element in ex.name_chain {
				if i > 0 {
					result += '/'
				}
				match element {
					Identifier {
						result += element.name
					}
					Macro {
						result += '<'
						if element.referable {
							result += '&'
						}
						result += unparse_one(Expr(element.ident_chain))
						result += '>'
					}
				}
			}
			result += '('
			for i, arg in ex.args {
				if i > 0 {
					result += ', '
				}
				result += unparse_one(arg)
			}
			result += ')'
			return result
		}
		FunctionDefinition {
			result += 'fn ' + ex.ident.name + '('
			for i, arg in ex.args {
				if i > 0 {
					result += ', '
				}
				result += match arg.source {
					.register { 'reg ' }
					.data { 'data ' }
					.effemeral { 'eff ' }
				}
				result += arg.name.name + ': '
				result += unparse_type(arg.arg_type)
			}
			result += '): '
			result += unparse_type(ex.return_type)
			result += ' {\n'
			for expr in ex.block.exprs {
				result += '  ' + unparse_one(expr) + ';\n'
			}
			result += '}'
			return result
		}
		FunctionInlineDefinition {
			result += ex.ident.name + '<'
			for i, arg in ex.args {
				if i > 0 {
					result += ', '
				}
				result += match arg.source {
					.register { 'reg ' }
					.data { 'data ' }
					.effemeral { 'eff ' }
				}
				result += arg.name.name + ': '
				result += unparse_type(arg.arg_type)
				result += ' ' + match arg.op {
					.assign { '=' }
					.store { '<-' }
					else { panic('invalid op') }
				} + ' '
				result += unparse_one(arg.value)
			}
			result += '> {\n'
			for expr in ex.block.exprs {
				result += '  ' + unparse_one(expr) + ';\n'
			}
			result += '}'
			return result
		}
		Macro {
			result += '<'
			if ex.referable {
				result += '&'
			}
			result += unparse_one(Expr(ex.ident_chain))
			result += '>'
			return result
		}
		IdentifierChain {
			match ex.kind {
				.identchk_ident {
					result += unparse_one(Expr(ex.ident or { panic('no unparse') }))
				}
				.identchk_macro {
					result += unparse_one(Expr(ex.macro or { panic('no unparse') }))
				}
				.identchk_array_integer {
					result += '['
					result += (ex.array_integer or { panic('no unparse') }).str()
					result += ']'
				}
				.identchk_integer {
					result += (ex.integer or { panic('no unparse') }).str()
				}
				.identchk_array_macro {
					result += '['
					result += unparse_one(Expr(ex.macro or { panic('no unparse') }))
					result += ']'
				}
			}
			if ex.next != none {
				if (*ex.next).kind !in [.identchk_array_integer, .identchk_array_macro] {
					result += '.'
				}
				result += unparse_one(Expr(*ex.next))
			}
			return result
		}
		Identifier {
			result += ex.name
			return result
		}
		Define {
			result += match ex.source {
				.register { 'reg ' }
				.data { 'data ' }
				.effemeral { 'eff ' }
			}
			result += ex.name.name
			result += ' := '
			result += unparse_one(ex.value)
			return result
		}
		BinaryExpr {
			result += unparse_one(ex.left)
			result += ' ' + match ex.operator {
				.eq { '==' }
				.assign { '=' }
				.plus { '+' }
				.minus { '-' }
				.star { '*' }
				.slash { '/' }
				.percent { '%' }
				.dotdot { '..' }
				.lte { '<=' }
				.gte { '>=' }
				.ne { '!=' }
				.plus_assign { '+=' }
				.minus_assign { '-=' }
				.star_assign { '*=' }
				.slash_assign { '/=' }
				.percent_assign { '%=' }
				.swap { '><' }
				else { panic('unhandled binary operator for unparse') }
			} + ' '
			result += unparse_one(ex.right)
			return result
		}
		Integer {
			result += ex.value.str()
			return result
		}
		String {
			result += '"' + ex.value + '"'
			return result
		}
		Char {
			result += "'" + ex.value + "'"
			return result
		}
		List {
			result += '['
			for i, entry in ex.entries {
				if i > 0 {
					result += ', '
				}
				result += unparse_one(entry)
			}
			result += ']'
			return result
		}
		Dictionary {
			result += '{'
			for i, entry in ex.entries {
				if i > 0 {
					result += ', '
				}
				match entry.kind {
					.dictentk_string {
						result += '"' + (entry.str or { panic('no unparse') }).value + '"'
					}
					.dictentk_integer {
						result += (entry.integer or { panic('no unparse') }).str()
					}
					.dictentk_macro {
						result += unparse_one(Expr(entry.macro or { panic('no unparse') }))
					}
				}
				result += ': ' + unparse_one(entry.value)
			}
			result += '}'
			return result
		}
		Range {
			if start := ex.start {
				result += unparse_one(start)
			}
			result += '..'
			if end := ex.end {
				result += unparse_one(end)
			}
			return result
		}
		Store {
			result += unparse_one(ex.left)
			result += ' <- '
			result += unparse_one(ex.right)
			return result
		}
		StringSlice {
			result += unparse_one(ex.target)
			result += '['
			if start := ex.start {
				result += unparse_one(start)
			}
			result += ':'
			if end := ex.end {
				result += unparse_one(end)
			}
			result += ']'
			return result
		}
		IndexExpr {
			result += unparse_one(ex.target)
			result += '['
			result += unparse_one(ex.index)
			result += ']'
			return result
		}
		NamespaceDefinition {
			result += 'namespace ' + ex.name.name + ' = {\n'
			for expr in ex.block.exprs {
				result += '  ' + unparse_one(expr) + ';\n'
			}
			result += '}'
			return result
		}
		NamespaceImport {
			result += 'namespace ' + ex.name.name + ' = "' + ex.path.value + '"'
			return result
		}
		NamespaceAlias {
			result += 'namespace ' + ex.name.name
			return result
		}
		ReturnStatement {
			result += 'return'
			if val := ex.value {
				result += ' ' + unparse_one(val)
			}
			return result
		}
		InterpolatedString {
			result += '"'
			for part in ex.parts {
				if part.is_macro {
					if m := part.macro {
						result += '<'
						if m.referable {
							result += '&'
						}
						result += unparse_one(Expr(m.ident_chain))
						result += '>'
					}
				} else {
					result += part.text
				}
			}
			result += '"'
			return result
		}
		QualifiedIdentifier {
			for i, ns in ex.namespace_path {
				if i > 0 {
					result += '.'
				}
				result += ns.name
			}
			if ex.namespace_path.len > 0 {
				result += '.'
			}
			result += ex.name.name
			return result
		}
		ReferenceExpr {
			result += '&' + unparse_one(ex.target)
			return result
		}
		else {
			print(ex)
		}
	}
	return result
}

fn unparse(exprs []Expr) string {
	mut result := ''
	for ex in exprs {
		result += unparse_one(ex)
	}
	return result
}
