// parse.v
// This is the parser
module main

import os

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
		.assign, .plus_assign, .minus_assign, .star_assign, .slash_assign, .percent_assign, .store { 0 }
		.eq, .ne, .lte, .gte, .lcarrot, .rcarrot, .swap { 2 }
		.plus, .minus { 3 }
		.star, .slash, .percent { 4 }
		else { 0 }
	}
}

struct Node {
mut:
	pos int
}

pub type Stmt = Define
	| TypedDefine
	| Assignment
	| Store
	| IfStmt
	| StructDefinition
	| FunctionDefinition
	| FunctionInlineDefinition
	| NamespaceDefinition
	| NamespaceImport
	| NamespaceAlias
	| Return
	| ExprStmt
	| MacroLiteralCommand
	| Block

pub type Expr = BinaryExpr
	| UnaryExpr
	| Literal
	| Identifier
	| MacroExpr
	| AccessExpr
	| StructLiteral // Don't want to be able to write everywhere so not putting with literal.
	| QualifiedIdentifier

pub type Literal = IntegerLiteral
	| StringLiteral
	| CharLiteral
	| ListLiteral
	| DictionaryLiteral
	| RangeLiteral

pub type AccessExpr = MemberAccessExpr | IndexAccessExpr | FunctionCallExpr

pub type AccessChainElement = FieldAccessElement
	| IndexAccessElement
	| MacroAccessElement
	| DerefAccessElement

pub struct DerefAccessElement {
	Node
}

pub struct StructDefinition {
	Node
pub mut:
	name   Identifier
	fields []StructField
}

pub struct StructField {
	Node
pub mut:
	name       Identifier
	field_type Type
}

pub struct StructLiteral {
	Node
pub mut:
	struct_name QualifiedIdentifier
	fields      []StructFieldInit
}

pub struct StructFieldInit {
	Node
pub mut:
	name  Identifier
	value Expr
}

pub type IdentifierChainElement = IdentifierChainName
	| IdentifierChainIndex
	| IdentifierChainMacro
	| IdentifierChainDeref

pub struct IdentifierChainDeref {
	Node
}

pub struct MacroLiteralCommand {
	Node
pub mut:
	parts []MacroLiteralPart
}

pub type MacroLiteralPart = MacroLiteralText | MacroLiteralMacro | MacroLiteralString

pub struct MacroLiteralText {
	Node
mut:
	text string
}

pub struct MacroLiteralMacro {
	Node
mut:
	macro_expr MacroExpr
}

pub struct MacroLiteralString {
	Node
mut:
	str_literal StringLiteral
}

pub struct Block {
	Node
pub mut:
	stmts []Stmt
}

pub struct Define {
	Node
mut:
	source ValueType
	name   Identifier
	value  Expr
}

// We are not going to let there be any macros in this definition
pub struct TypedDefine {
	Node
mut:
	source ValueType
	name   Identifier
	typ    Type
}

pub struct Assignment {
	Node
mut:
	left     Expr
	operator TokenKind
	right    Expr
}

pub struct Store {
	Node
mut:
	left  Expr
	right Expr
}

pub struct IfStmt {
	Node
mut:
	condition  Expr
	then_block Block
	else_block ?Block
}

pub struct FunctionArgument {
	Node
mut:
	source   ValueType
	name     Identifier
	arg_type Type
}

pub struct FunctionInlineArgument {
	Node
mut:
	source   ValueType
	name     Identifier
	arg_type Type
	op       TokenKind
	value    Expr
}

pub struct FunctionDefinition {
	Node
mut:
	ident       Identifier
	args        []FunctionArgument
	return_type Type
	block       Block
}

pub struct FunctionInlineDefinition {
	Node
mut:
	ident Identifier
	args  []FunctionInlineArgument
	block Block
}

pub struct NamespaceDefinition {
	Node
mut:
	name  Identifier
	block Block
}

pub struct NamespaceImport {
	Node
mut:
	name Identifier
	path StringLiteral
}

pub struct NamespaceAlias {
	Node
mut:
	name  Identifier
	block Block
}

pub struct Return {
	Node
mut:
	value ?Expr
}

pub struct ExprStmt {
	Node
mut:
	expr Expr
}

pub struct IntegerLiteral {
	Node
mut:
	value int
}

pub struct StringLiteral {
	Node
mut:
	value        string
	interpolated bool
	parts        []InterpolatedStringPart
}

pub struct InterpolatedStringPart {
mut:
	is_macro   bool
	text       string
	macro_expr ?MacroExpr
}

pub struct CharLiteral {
	Node
mut:
	value string
}

pub struct ListLiteral {
	Node
mut:
	elements []Expr
}

pub enum DictionaryKeyKind {
	integer_key
	string_key
	macro_key
}

pub struct DictionaryEntry {
	Node
mut:
	key_kind    DictionaryKeyKind
	integer_key ?int
	string_key  ?StringLiteral
	macro_key   ?MacroExpr
	value       Expr
}

pub struct DictionaryLiteral {
	Node
mut:
	entries []DictionaryEntry
}

pub struct RangeLiteral {
	Node
mut:
	start ?Expr
	end   ?Expr
}

pub struct Identifier {
	Node
mut:
	name string
}

pub struct MacroExpr {
	Node
mut:
	referable   bool
	ident_chain IdentifierChain
}

pub struct UnaryExpr {
	Node
mut:
	operator TokenKind
	right    Expr
}

pub struct BinaryExpr {
	Node
mut:
	left     Expr
	operator TokenKind
	right    Expr
}

pub struct MemberAccessExpr {
	Node
mut:
	target Expr
	chain  []AccessChainElement
}

pub struct FieldAccessElement {
	Node
mut:
	field Identifier
}

pub struct IndexAccessElement {
	Node
mut:
	index_expr Expr
	is_slice   bool
	slice_end  ?Expr
}

pub struct MacroAccessElement {
	Node
mut:
	macro_expr MacroExpr
}

pub struct IdentifierChain {
	Node
mut:
	elements []IdentifierChainElement
}

pub struct IdentifierChainName {
	Node
mut:
	name Identifier
}

pub struct IdentifierChainIndex {
	Node
mut:
	index_expr Expr
}

pub struct IdentifierChainMacro {
	Node
mut:
	macro_expr MacroExpr
}

// Added missing struct
pub struct IndexAccessExpr {
	Node
mut:
	target Expr
	index  IndexAccessElement
}

pub struct FunctionCallExpr {
	Node
mut:
	base_target Expr
	args        []Expr
}

pub struct QualifiedIdentifier {
	Node
mut:
	namespace_path []Identifier
	name           Identifier
}

pub fn (q QualifiedIdentifier) to_list() []string {
	mut result := []string{}
	for n in q.namespace_path {
		result << n.name
	}
	return result
}

pub type Type = BuiltinType | ReferenceType | StructType

pub enum BuiltinType {
	int_t
	float_t
	list_t
	string_t
	dict_t
	void_t
}

pub struct ReferenceType {
	Node
mut:
	base Type
}

pub struct StructType {
	Node
pub mut:
	name QualifiedIdentifier
}

pub enum ValueType {
	effemeral
	register
	data
}

pub fn (v ValueType) to_ir() StorageKind {
	match v {
		.data { return .data }
		.register { return .register }
		.effemeral { return .effemeral }
	}
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
	pos := p.current.pos
	cur := p.current
	if p.current.kind != .ident {
		panic('was expecting ident at pos ${pos}')
	}
	p.advance()
	return Identifier{
		Node: Node{
			pos: pos
		}
		name: cur.str
	}
}

fn (mut p Parser) parse_type() Type {
	pos := p.current.pos
	if p.current.kind == .ampersand {
		p.consume(.ampersand)
		base_type := p.parse_type()
		return Type(ReferenceType{
			Node: Node{
				pos: pos
			}
			base: base_type
		})
	}
	match p.current.kind {
		.kw_int {
			p.advance()
			return Type(BuiltinType.int_t)
		}
		.kw_float {
			p.advance()
			return Type(BuiltinType.float_t)
		}
		.kw_list {
			p.advance()
			return Type(BuiltinType.list_t)
		}
		.kw_string {
			p.advance()
			return Type(BuiltinType.string_t)
		}
		.kw_dict {
			p.advance()
			return Type(BuiltinType.dict_t)
		}
		.kw_void {
			p.advance()
			return Type(BuiltinType.void_t)
		}
		.ident {
			// Struct type or custom type
			mut namespace_path := []Identifier{}
			namespace_path << p.parse_ident()

			for p.current.kind == .dot && p.next.kind == .ident {
				p.consume(.dot)
				namespace_path << p.parse_ident()
			}

			if namespace_path.len > 1 {
				name := namespace_path.pop()
				return Type(StructType{
					Node: Node{
						pos: pos
					}
					name: QualifiedIdentifier{
						Node:           Node{
							pos: pos
						}
						namespace_path: namespace_path
						name:           name
					}
				})
			} else {
				return Type(StructType{
					Node: Node{
						pos: pos
					}
					name: QualifiedIdentifier{
						Node:           Node{
							pos: pos
						}
						namespace_path: []
						name:           namespace_path[0]
					}
				})
			}
		}
		else {
			panic('expected type keyword at pos ${pos}')
		}
	}
}

fn (mut p Parser) parse_struct_definition() Stmt {
	pos := p.current.pos
	p.consume(.kw_struct)

	if p.current.kind != .ident {
		panic('expected identifier after "struct" at pos ${p.current.pos}')
	}
	name := p.parse_ident()

	p.consume(.lcurly)

	mut fields := []StructField{}

	for p.current.kind != .rcurly && p.current.kind != .eof {
		field_pos := p.current.pos

		if p.current.kind != .ident {
			panic('expected field name at pos ${p.current.pos}')
		}
		field_name := p.parse_ident()

		p.consume(.colon)

		field_type := p.parse_type()

		fields << StructField{
			Node:       Node{
				pos: field_pos
			}
			name:       field_name
			field_type: field_type
		}

		if p.current.kind == .comma {
			p.consume(.comma)
		} else if p.current.kind != .rcurly {
			// Allow optional trailing comma or semicolon
			if p.current.kind == .semicolon {
				p.consume(.semicolon)
			}
		}
	}

	p.consume(.rcurly)

	return Stmt(StructDefinition{
		Node:   Node{
			pos: pos
		}
		name:   name
		fields: fields
	})
}

fn (mut p Parser) parse_struct_literal(struct_name QualifiedIdentifier) Expr {
	pos := p.current.pos
	p.consume(.lcurly)

	mut field_inits := []StructFieldInit{}

	if p.current.kind != .rcurly {
		for {
			field_pos := p.current.pos

			if p.current.kind != .ident {
				panic('expected field name in struct initialization at pos ${p.current.pos}')
			}
			field_name := p.parse_ident()

			p.consume(.colon)

			field_value := p.parse_expr(0)

			field_inits << StructFieldInit{
				Node:  Node{
					pos: field_pos
				}
				name:  field_name
				value: field_value
			}

			if p.current.kind == .comma {
				p.consume(.comma)
				if p.current.kind == .rcurly {
					break
				}
			} else {
				break
			}
		}
	}

	p.consume(.rcurly)

	return Expr(StructLiteral{
		Node:        Node{
			pos: pos
		}
		struct_name: struct_name
		fields:      field_inits
	})
}

fn (mut p Parser) parse_definition() Stmt {
	pos := p.current.pos
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
			panic('illegal state in parse_definition')
		}
	}
	p.advance()

	if p.current.kind != .ident {
		panic('expected identifier after source keyword at pos ${pos}')
	}
	name := p.parse_ident()

	if p.current.kind == .define {
		p.advance()
		value := p.parse_expr(0)
		return Stmt(Define{
			Node:   Node{
				pos: pos
			}
			source: source
			name:   name
			value:  value
		})
	} else if p.current.kind == .colon {
		// Declaration with type: data n : Node;
		p.consume(.colon)
		typ := p.parse_type()

		return Stmt(TypedDefine{
			Node:   Node{
				pos: pos
			}
			source: source
			name:   name
			typ:    typ
		})
	}
	panic('was expecting := or : after identifier at pos ${p.current.pos}')
}

fn (mut p Parser) parse_identifier_chain_start() IdentifierChain {
	pos := p.current.pos
	mut elements := []IdentifierChainElement{}

	match p.current.kind {
		.ident {
			ident := p.parse_ident()
			elements << IdentifierChainElement(IdentifierChainName{
				Node: Node{
					pos: ident.pos
				}
				name: ident
			})
		}
		.lcarrot {
			macro := p.parse_macro()
			elements << IdentifierChainElement(IdentifierChainMacro{
				Node:       Node{
					pos: macro.pos
				}
				macro_expr: macro
			})
		}
		.lit_integer {
			val := p.current.str.int()
			ipos := p.current.pos
			p.advance()
			elements << IdentifierChainElement(IdentifierChainIndex{
				Node:       Node{
					pos: ipos
				}
				index_expr: Expr(Literal(IntegerLiteral{
					Node:  Node{
						pos: ipos
					}
					value: val
				}))
			})
		}
		else {
			panic('unexpected token in identifier chain at pos ${p.current.pos}')
		}
	}

	for {
		if p.current.kind == .lsquare {
			p.consume(.lsquare)
			idx_pos := p.current.pos
			match p.current.kind {
				.lit_integer {
					val := p.current.str.int()
					p.advance()
					elements << IdentifierChainElement(IdentifierChainIndex{
						Node:       Node{
							pos: idx_pos
						}
						index_expr: Expr(Literal(IntegerLiteral{
							Node:  Node{
								pos: idx_pos
							}
							value: val
						}))
					})
				}
				.lcarrot {
					macro := p.parse_macro()
					elements << IdentifierChainElement(IdentifierChainIndex{
						Node:       Node{
							pos: idx_pos
						}
						index_expr: Expr(macro)
					})
				}
				else {
					panic('expected integer or macro in array access at pos ${idx_pos}')
				}
			}
			p.consume(.rsquare)
		} else if p.current.kind == .dot {
			p.consume(.dot)
			dot_pos := p.current.pos

			// Check for dereference (@)
			if p.current.kind == .at {
				p.consume(.at)
				elements << IdentifierChainElement(IdentifierChainDeref{
					Node: Node{
						pos: dot_pos
					}
				})
				continue
			}

			match p.current.kind {
				.ident {
					ident := p.parse_ident()
					elements << IdentifierChainElement(IdentifierChainName{
						Node: Node{
							pos: dot_pos
						}
						name: ident
					})
				}
				.lcarrot {
					macro := p.parse_macro()
					elements << IdentifierChainElement(IdentifierChainMacro{
						Node:       Node{
							pos: dot_pos
						}
						macro_expr: macro
					})
				}
				.lit_integer {
					val := p.current.str.int()
					ipos := p.current.pos
					p.advance()
					elements << IdentifierChainElement(IdentifierChainIndex{
						Node:       Node{
							pos: ipos
						}
						index_expr: Expr(Literal(IntegerLiteral{
							Node:  Node{
								pos: ipos
							}
							value: val
						}))
					})
				}
				else {
					panic('expected identifier, macro, or integer after dot at pos ${dot_pos}')
				}
			}
		} else {
			break
		}
	}

	return IdentifierChain{
		Node:     Node{
			pos: pos
		}
		elements: elements
	}
}

fn (mut p Parser) parse_macro() MacroExpr {
	pos := p.current.pos
	p.consume(.lcarrot)
	referable := p.consume_silent(.ampersand)
	ident_chain := p.parse_identifier_chain_start()
	p.consume(.rcarrot)
	return MacroExpr{
		Node:        Node{
			pos: pos
		}
		referable:   referable
		ident_chain: ident_chain
	}
}

fn (mut p Parser) parse_dictionary_entry() DictionaryEntry {
	pos := p.current.pos
	mut result := DictionaryEntry{
		Node: Node{
			pos: pos
		}
	}

	match p.current.kind {
		.lit_string {
			result.key_kind = .string_key
			str_pos := p.current.pos
			result.string_key = StringLiteral{
				Node:  Node{
					pos: str_pos
				}
				value: p.current.str
			}
			p.advance()
		}
		.lit_integer {
			result.key_kind = .integer_key
			result.integer_key = p.current.str.int()
			p.advance()
		}
		.lcarrot {
			result.key_kind = .macro_key
			result.macro_key = p.parse_macro()
		}
		else {
			panic('illegal state when parsing dictionary entry at pos ${pos}')
		}
	}

	p.consume(.colon)
	result.value = p.parse_expr(0)
	return result
}

fn (mut p Parser) parse_dictionary_literal(pos int) Literal {
	p.consume(.lcurly)
	if p.current.kind == .rcurly {
		p.consume(.rcurly)
		return Literal(DictionaryLiteral{
			Node:    Node{
				pos: pos
			}
			entries: []DictionaryEntry{}
		})
	}
	mut result := []DictionaryEntry{}
	for p.current.kind != .rcurly {
		if p.current.kind == .comma {
			panic('illegal state: unexpected comma in dictionary at pos ${p.current.pos}')
		}
		result << p.parse_dictionary_entry()
		if p.current.kind == .comma {
			p.consume(.comma)
		}
	}
	p.consume(.rcurly)
	return Literal(DictionaryLiteral{
		Node:    Node{
			pos: pos
		}
		entries: result
	})
}

fn (mut p Parser) parse_list_literal(pos int) Literal {
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
	return Literal(ListLiteral{
		Node:     Node{
			pos: pos
		}
		elements: entries
	})
}

fn (mut p Parser) parse_range_literal(pos int, left ?Expr) Literal {
	p.consume(.dotdot)

	mut end_expr := ?Expr(none)
	if p.current.kind !in [.rparen, .semicolon, .comma, .rsquare] {
		end_expr = p.parse_expr(0)
	}

	return Literal(RangeLiteral{
		Node:  Node{
			pos: pos
		}
		start: left
		end:   end_expr
	})
}

fn (mut p Parser) parse_interpolated_string_literal(pos int, str_value string) Literal {
	mut parts := []InterpolatedStringPart{}
	mut current_text := ''
	mut i := 1

	for i < str_value.len - 1 {
		if str_value[i] == `<` {
			if current_text.len > 0 {
				parts << InterpolatedStringPart{
					is_macro: false
					text:     current_text
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
					is_macro:   true
					text:       ''
					macro_expr: parsed_macro
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
		}
	}

	return Literal(StringLiteral{
		Node:         Node{
			pos: pos
		}
		value:        str_value
		interpolated: true
		parts:        parts
	})
}

fn (mut p Parser) parse_literal_expr() Literal {
	pos := p.current.pos
	match p.current.kind {
		.lit_integer {
			val := p.current.str.int()
			p.advance()
			return Literal(IntegerLiteral{
				Node:  Node{
					pos: pos
				}
				value: val
			})
		}
		.lit_string {
			val := p.current.str
			p.advance()
			if val.contains('<') && val.contains('>') {
				return p.parse_interpolated_string_literal(pos, val)
			}
			return Literal(StringLiteral{
				Node:  Node{
					pos: pos
				}
				value: val
			})
		}
		.lit_char {
			val := p.current.str
			p.advance()
			return Literal(CharLiteral{
				Node:  Node{
					pos: pos
				}
				value: val
			})
		}
		.lsquare {
			return p.parse_list_literal(pos)
		}
		.lcurly {
			return p.parse_dictionary_literal(pos)
		}
		.dotdot {
			return p.parse_range_literal(pos, none)
		}
		else {
			panic('expected literal at pos ${pos}')
		}
	}
}

fn (mut p Parser) parse_function_argument() FunctionArgument {
	pos := p.current.pos
	mut source := ValueType.effemeral
	match p.current.kind {
		.kw_reg { source = .register }
		.kw_data { source = .data }
		.kw_eff { source = .effemeral }
		else { panic('expected argument source: reg | data | eff at pos ${pos}') }
	}
	p.advance()

	if p.current.kind != .ident {
		panic('expected identifier for function argument name at pos ${p.current.pos}')
	}
	name := p.parse_ident()
	p.consume(.colon)
	arg_type := p.parse_type()

	return FunctionArgument{
		Node:     Node{
			pos: pos
		}
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

fn (mut p Parser) parse_function_definition() Stmt {
	pos := p.current.pos
	p.consume(.kw_fn)

	if p.current.kind != .ident {
		panic('expected function identifier after "fn" at pos ${p.current.pos}')
	}
	ident := p.parse_ident()

	args := p.parse_function_arguments()

	p.consume(.colon)

	ret_ty := p.parse_type()

	block := p.parse_block()

	return Stmt(FunctionDefinition{
		Node:        Node{
			pos: pos
		}
		ident:       ident
		args:        args
		return_type: ret_ty
		block:       block
	})
}

fn (mut p Parser) parse_function_inline_argument() FunctionInlineArgument {
	pos := p.current.pos
	mut source := ValueType.effemeral
	match p.current.kind {
		.kw_reg { source = .register }
		.kw_data { source = .data }
		.kw_eff { source = .effemeral }
		else { panic('expected argument source at pos ${pos}') }
	}
	p.advance()

	if p.current.kind != .ident {
		panic('expected identifier for inline arg at pos ${p.current.pos}')
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
			panic('expected "=" or "<-" for inline function arg at pos ${p.current.pos}')
		}
	}

	value := p.parse_expr(0)

	return FunctionInlineArgument{
		Node:     Node{
			pos: pos
		}
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

fn (mut p Parser) parse_function_inline_definition() Stmt {
	pos := p.current.pos
	if p.current.kind != .ident {
		panic('expected function identifier for inline function at pos ${pos}')
	}
	ident := p.parse_ident()

	args := p.parse_function_inline_arguments()

	block := p.parse_block()

	return Stmt(FunctionInlineDefinition{
		Node:  Node{
			pos: pos
		}
		ident: ident
		args:  args
		block: block
	})
}

fn (mut p Parser) parse_if_statement() Stmt {
	pos := p.current.pos
	p.consume(.kw_if)
	p.consume(.lparen)

	condition := p.parse_expr(0)

	p.consume(.rparen)

	then_block := p.parse_block()

	mut else_block := ?Block(none)
	if p.current.kind == .kw_else {
		p.consume(.kw_else)
		else_block = p.parse_block()
	}

	return Stmt(IfStmt{
		Node:       Node{
			pos: pos
		}
		condition:  condition
		then_block: then_block
		else_block: else_block
	})
}

fn (mut p Parser) parse_qualified_identifier() Expr {
	pos := p.current.pos
	mut namespace_path := []Identifier{}

	namespace_path << p.parse_ident()

	for p.current.kind == .dot && p.next.kind == .ident {
		p.consume(.dot)
		namespace_path << p.parse_ident()
	}

	if namespace_path.len > 1 {
		name := namespace_path.pop()
		return Expr(QualifiedIdentifier{
			Node:           Node{
				pos: pos
			}
			namespace_path: namespace_path
			name:           name
		})
	}

	// Just a single identifier
	return Expr(namespace_path[0])
}

fn (mut p Parser) parse_function_call(base_target Expr) Expr {
	pos := p.current.pos
	mut args := []Expr{}

	p.consume(.lparen)

	if p.current.kind != .rparen {
		args << p.parse_expr(0)

		for p.current.kind == .comma {
			p.consume(.comma)
			if p.current.kind == .rparen {
				break
			}
			args << p.parse_expr(0)
		}
	}

	p.consume(.rparen)

	return Expr(AccessExpr(FunctionCallExpr{
		Node:        Node{
			pos: pos
		}
		base_target: base_target
		args:        args
	}))
}

fn (mut p Parser) parse_rest_as_block() Block {
	mut result := Block{
		Node: Node{p.current.pos}
	}

	for p.current.kind != .eof {
		result.stmts << p.parse_statement()
	}

	return result
}

fn (mut p Parser) parse_namespace() Stmt {
	pos := p.current.pos
	p.consume(.kw_namespace)

	if p.current.kind != .ident {
		panic('expected identifier after namespace at pos ${p.current.pos}')
	}
	name := p.parse_ident()

	if p.current.kind == .semicolon {
		p.consume(.semicolon)
		return Stmt(NamespaceAlias{
			Node:  Node{
				pos: pos
			}
			name:  name
			block: p.parse_rest_as_block()
		})
	}

	p.consume(.assign)

	if p.current.kind == .lit_string {
		str_pos := p.current.pos
		path := StringLiteral{
			Node:  Node{
				pos: str_pos
			}
			value: p.current.str
		}
		p.advance()
		p.consume(.semicolon)
		return Stmt(NamespaceImport{
			Node: Node{
				pos: pos
			}
			name: name
			path: path
		})
	} else if p.current.kind == .lcurly {
		block := p.parse_block()
		p.consume(.semicolon)
		return Stmt(NamespaceDefinition{
			Node:  Node{
				pos: pos
			}
			name:  name
			block: block
		})
	}

	panic('expected string or block after namespace = at pos ${p.current.pos}')
}

fn (mut p Parser) parse_return_statement() Stmt {
	pos := p.current.pos
	p.consume(.kw_return)

	if p.current.kind == .semicolon {
		return Stmt(Return{
			Node:  Node{
				pos: pos
			}
			value: none
		})
	}

	value := p.parse_expr(0)

	return Stmt(Return{
		Node:  Node{
			pos: pos
		}
		value: value
	})
}

fn (mut p Parser) parse_prefix() Expr {
	pos := p.current.pos
	match p.current.kind {
		.ampersand, .at, .plus, .minus {
			op := p.current.kind
			p.advance()
			right := p.parse_expr(5)
			return Expr(UnaryExpr{
				Node:     Node{
					pos: pos
				}
				operator: op
				right:    right
			})
		}
		.ident {
			saved := p.save_lexer_state()
			first_ident := p.parse_ident()

			// Check for qualified identifier
			if p.current.kind == .dot && p.next.kind == .ident {
				mut namespace_path := []Identifier{}
				namespace_path << first_ident

				for p.current.kind == .dot && p.next.kind == .ident {
					p.consume(.dot)
					namespace_path << p.parse_ident()
				}

				// Check for struct literal
				if p.current.kind == .lcurly {
					name := namespace_path.pop()
					qualified_name := QualifiedIdentifier{
						Node:           Node{
							pos: pos
						}
						namespace_path: if namespace_path.len > 0 { namespace_path } else { [] }
						name:           name
					}
					return p.parse_struct_literal(qualified_name)
				}

				// Just a qualified identifier
				p.restore_lexer_state(saved)
				return p.parse_qualified_identifier()
			}

			// Check for struct literal with simple name
			if p.current.kind == .lcurly {
				qualified_name := QualifiedIdentifier{
					Node:           Node{
						pos: pos
					}
					namespace_path: []
					name:           first_ident
				}
				return p.parse_struct_literal(qualified_name)
			}

			// Check for function call
			if p.current.kind == .lparen {
				p.restore_lexer_state(saved)
				target := p.parse_ident()
				return p.parse_function_call(Expr(target))
			}

			// Just an identifier
			return Expr(first_ident)
		}
		.lcarrot {
			return Expr(p.parse_macro())
		}
		.lparen {
			p.consume(.lparen)
			expr := p.parse_expr(0)
			p.consume(.rparen)
			return expr
		}
		.lit_integer, .lit_string, .lit_char, .lsquare, .lcurly, .dotdot {
			return Expr(p.parse_literal_expr())
		}
		else {
			panic('unexpected token in prefix position: ${p.current.kind} at pos ${pos}')
		}
	}
}

fn (mut p Parser) parse_index_or_slice_access(pos int, left Expr) Expr {
	p.consume(.lsquare)

	if p.current.kind == .colon {
		mut end_expr := ?Expr(none)
		p.consume(.colon)
		if p.current.kind != .rsquare {
			end_expr = p.parse_expr(0)
		}
		p.consume(.rsquare)
		return Expr(AccessExpr(IndexAccessExpr{
			Node:   Node{
				pos: pos
			}
			target: left
			index:  IndexAccessElement{
				Node:       Node{
					pos: pos
				}
				index_expr: Expr(Literal(IntegerLiteral{
					Node:  Node{
						pos: pos
					}
					value: 0
				}))
				is_slice:   true
				slice_end:  end_expr
			}
		}))
	}

	if p.current.kind == .rsquare {
		p.consume(.rsquare)
		return Expr(AccessExpr(IndexAccessExpr{
			Node:   Node{
				pos: pos
			}
			target: left
			index:  IndexAccessElement{
				Node:       Node{
					pos: pos
				}
				index_expr: Expr(Literal(IntegerLiteral{
					Node:  Node{
						pos: pos
					}
					value: 0
				}))
				is_slice:   true
				slice_end:  none
			}
		}))
	}

	first_expr := p.parse_expr(0)

	if p.current.kind == .colon {
		p.consume(.colon)
		mut end_expr := ?Expr(none)
		if p.current.kind != .rsquare {
			end_expr = p.parse_expr(0)
		}
		p.consume(.rsquare)
		return Expr(AccessExpr(IndexAccessExpr{
			Node:   Node{
				pos: pos
			}
			target: left
			index:  IndexAccessElement{
				Node:       Node{
					pos: pos
				}
				index_expr: first_expr
				is_slice:   true
				slice_end:  end_expr
			}
		}))
	} else {
		p.consume(.rsquare)
		return Expr(AccessExpr(IndexAccessExpr{
			Node:   Node{
				pos: pos
			}
			target: left
			index:  IndexAccessElement{
				Node:       Node{
					pos: pos
				}
				index_expr: first_expr
				is_slice:   false
			}
		}))
	}
}

fn (mut p Parser) parse_postfix(mut left Expr) Expr {
	for {
		pos := p.current.pos
		match p.current.kind {
			.lsquare {
				left = p.parse_index_or_slice_access(pos, left)
			}
			.dot {
				p.consume(.dot)

				// Check for dereference
				if p.current.kind == .at {
					p.consume(.at)
					left = Expr(AccessExpr(MemberAccessExpr{
						Node:   Node{
							pos: pos
						}
						target: left
						chain:  [
							AccessChainElement(DerefAccessElement{
								Node: Node{
									pos: pos
								}
							}),
						]
					}))
					continue
				}

				match p.current.kind {
					.ident {
						field := p.parse_ident()
						left = Expr(AccessExpr(MemberAccessExpr{
							Node:   Node{
								pos: pos
							}
							target: left
							chain:  [
								AccessChainElement(FieldAccessElement{
									Node:  Node{
										pos: field.pos
									}
									field: field
								}),
							]
						}))
					}
					.lcarrot {
						macro := p.parse_macro()
						left = Expr(AccessExpr(MemberAccessExpr{
							Node:   Node{
								pos: pos
							}
							target: left
							chain:  [
								AccessChainElement(MacroAccessElement{
									Node:       Node{
										pos: macro.pos
									}
									macro_expr: macro
								}),
							]
						}))
					}
					else {
						panic('expected identifier or macro after dot at pos ${pos}')
					}
				}
			}
			.lparen {
				left = p.parse_function_call(left)
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
			pos := p.current.pos
			left = Expr(p.parse_range_literal(pos, left))
			continue
		}

		prec := p.current.kind.precidence()
		if prec <= min_prec {
			break
		}
		pos := p.current.pos
		op := p.current.kind
		p.advance()

		right := p.parse_expr(prec)
		left = Expr(BinaryExpr{
			Node:     Node{
				pos: pos
			}
			left:     left
			operator: op
			right:    right
		})
	}
	return left
}

fn (mut p Parser) parse_statement() Stmt {
	pos := p.current.pos
	match p.current.kind {
		.dollar {
			stmt := p.parse_macro_literal_command()
			p.consume(.semicolon)
			return stmt
		}
		.kw_struct {
			stmt := p.parse_struct_definition()
			p.consume(.semicolon)
			return stmt
		}
		.kw_return {
			ret := p.parse_return_statement()
			p.consume(.semicolon)
			return ret
		}
		.kw_namespace {
			ns := p.parse_namespace()
			return ns
		}
		.kw_fn {
			return p.parse_function_definition()
		}
		.kw_if {
			return p.parse_if_statement()
		}
		.kw_reg, .kw_data, .kw_eff {
			if p.next.kind == .ident {
				saved := p.save_lexer_state()

				p.advance()
				p.advance()

				if p.current.kind == .lcarrot {
					p.restore_lexer_state(saved)
					return p.parse_function_inline_definition()
				} else {
					p.restore_lexer_state(saved)
					def := p.parse_definition()
					p.consume(.semicolon)
					return def
				}
			}
			def := p.parse_definition()
			p.consume(.semicolon)
			return def
		}
		else {
			left := p.parse_expr(0)

			match p.current.kind {
				.assign, .plus_assign, .minus_assign, .star_assign, .slash_assign, .percent_assign,
				.eq, .ne, .lte, .gte, .lcarrot, .rcarrot, .swap, .plus, .minus, .star, .slash,
				.percent {
					op := p.current.kind
					p.advance()
					right := p.parse_expr(0)
					p.consume(.semicolon)
					return Stmt(Assignment{
						Node:     Node{
							pos: pos
						}
						left:     left
						operator: op
						right:    right
					})
				}
				.store {
					p.advance()
					right := p.parse_expr(0)
					p.consume(.semicolon)
					return Stmt(Store{
						Node:  Node{
							pos: pos
						}
						left:  left
						right: right
					})
				}
				else {}
			}

			p.consume(.semicolon)
			return Stmt(ExprStmt{
				Node: Node{
					pos: pos
				}
				expr: left
			})
		}
	}
}

fn (mut p Parser) parse_macro_literal_command() Stmt {
	pos := p.current.pos
	p.consume(.dollar)

	mut parts := []MacroLiteralPart{}

	for p.current.kind != .semicolon && p.current.kind != .eof {
		match p.current.kind {
			.lcarrot {
				// Parse macro expression
				macro := p.parse_macro()
				parts << MacroLiteralPart(MacroLiteralMacro{
					Node:       Node{
						pos: macro.pos
					}
					macro_expr: macro
				})
			}
			.lit_string {
				// Parse string literal (may contain macros)
				str_pos := p.current.pos
				str_value := p.current.str
				p.advance()

				mut str_literal := StringLiteral{}
				if str_value.contains('<') && str_value.contains('>') {
					// Parse interpolated string
					lit := p.parse_interpolated_string_literal(str_pos, str_value)
					match lit {
						StringLiteral {
							str_literal = lit
						}
						else {
							panic('expected string literal at pos ${str_pos}')
						}
					}
				} else {
					// Plain string
					str_literal = StringLiteral{
						Node:         Node{
							pos: str_pos
						}
						value:        str_value
						interpolated: false
					}
				}

				parts << MacroLiteralPart(MacroLiteralString{
					Node:        Node{
						pos: str_pos
					}
					str_literal: str_literal
				})
			}
			else {
				// Raw literal text - accumulate all tokens until macro, string, or semicolon
				text_pos := p.current.pos
				mut text := ''
				mut first := true

				for p.current.kind !in [.lcarrot, .lit_string, .semicolon, .eof] {
					if !first {
						text += ' '
					}
					text += p.current.str
					first = false
					p.advance()
				}

				if text.len > 0 {
					parts << MacroLiteralPart(MacroLiteralText{
						Node: Node{
							pos: text_pos
						}
						text: text
					})
				}
			}
		}
	}

	return Stmt(MacroLiteralCommand{
		Node:  Node{
			pos: pos
		}
		parts: parts
	})
}

fn (mut p Parser) parse_block() Block {
	pos := p.current.pos
	p.consume(.lcurly)
	mut stmts := []Stmt{}
	for p.current.kind !in [.rcurly, .eof] {
		stmts << p.parse_statement()
	}
	p.consume(.rcurly)
	return Block{
		Node:  Node{
			pos: pos
		}
		stmts: stmts
	}
}

pub fn parse_file(path string) []Stmt {
	if !os.is_file(path) {
		panic('${path} is not a file...')
	}
	filedata := os.read_file(path) or { panic('could not read file ${path}') }
	ast := parse(filedata)
	return ast
}

pub fn parse(str string) []Stmt {
	mut p := Parser{
		lexer: Lexer{
			src:   str
			index: 0
		}
	}
	p.current = p.lexer.next_token()
	p.next = p.lexer.next_token()

	mut statements := []Stmt{}
	for p.current.kind != .eof {
		stmt := p.parse_statement()
		statements << stmt
	}
	return statements
}

pub struct LexerState {
pub mut:
	index   int
	current Token
	next    Token
}

fn (mut p Parser) save_lexer_state() LexerState {
	return LexerState{
		index:   p.lexer.index
		current: p.current
		next:    p.next
	}
}

fn (mut p Parser) restore_lexer_state(state LexerState) {
	p.lexer.index = state.index
	p.current = state.current
	p.next = state.next
}
