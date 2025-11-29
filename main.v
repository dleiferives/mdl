module main

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
