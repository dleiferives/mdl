module main

import datatypes

// gets it and ads it to the local map
pub fn (mut b IRBuilder) bb_get_reference(name string, bbid BBID, fid FID) (bool, IRRef) {
	mut bb := b.basic_blocks[bbid]
	mut fun := b.functions[fid]
	mut ns := b.namespaces[fun.namespace]

	// check if its in our arguments
	if name in bb.vars_map {
		return true, b.functions[fid].refs[bb.vars_map[name]]
	}
	for arg in bb.args {
		if arg.name == name {
			ref := IRRef{
				id:    b.functions[fid].refs.len
				value: arg
				typ:   arg.typ
			}
			b.functions[fid].refs << ref
			b.basic_blocks[bbid].vars_map[name] = ref.id
			return true, ref
		}
	}

	mut visited := map[BBID]bool{}
	visited[bbid] = true
	mut to_visit := datatypes.Queue[BBID]{}
	for pred in bb.predecessors {
		to_visit.push(pred)
	}

	// check if its defined in any predecesor blocks
	// who exist in the same scope TODO
	for !to_visit.is_empty() {
		new_id := to_visit.pop() or { continue }
		if new_id in visited {
			continue
		}
		new := b.basic_blocks[new_id]
		if name in new.vars_map {
			ref := new.vars_map[name]
			b.basic_blocks[bbid].vars_map[name] = ref
			return true, b.functions[fid].refs[ref]
		}
		visited[new_id] = true
		for pred in new.predecessors {
			to_visit.push(pred)
		}
	}

	// check if its in our owning namespace
	if name in ns.variable_map {
		ref := IRRef{
			id:    b.functions[fid].refs.len
			value: IRRefSum(ns.variable_map[name])
		}
		b.functions[fid].refs << ref
		b.basic_blocks[bbid].vars_map[name] = ref.id
		return true, ref
	}

	// check if its in our function's arguments
	for arg in b.functions[fid].args {
		if arg.name == name {
			ref := IRRef{
				id:    b.functions[fid].refs.len
				value: arg
				typ:   arg.typ
			}
			b.functions[fid].refs << ref
			b.basic_blocks[bbid].vars_map[name] = ref.id
			return true, ref
		}
	}
	return false, IRRef{}
}

pub enum RefLevel {
	invalid
	bb_args
	pred_block
	pred_args
	fun_args
	namespace
}

// Just looks for it does not add it anywhere
pub fn (mut b IRBuilder) bb_get_reference_no_creation(name string, bbid BBID, fid FID) (bool, IRRef, RefLevel) {
	mut bb := b.basic_blocks[bbid]
	mut fun := b.functions[fid]
	mut ns := b.namespaces[fun.namespace]

	// check if its in our arguments
	for arg in bb.args {
		if arg.name == name {
			ref := IRRef{
				id:    RID(-1)
				value: arg
				typ:   arg.typ
			}
			return true, ref, RefLevel.bb_args
		}
	}

	mut visited := map[BBID]bool{}
	visited[bbid] = true
	mut to_visit := datatypes.Queue[BBID]{}
	for pred in bb.predecessors {
		to_visit.push(pred)
	}

	// check if its defined in any predecesor blocks
	// who exist in the same scope TODO
	for !to_visit.is_empty() {
		new_id := to_visit.pop() or { continue }
		if new_id in visited {
			continue
		}
		new := b.basic_blocks[new_id]
		if name in new.vars_map {
			ref := new.vars_map[name]
			return true, fun.refs[ref], RefLevel.pred_block
		}

		for arg in new.args {
			if arg.name == name {
				ref := IRRef{
					id:    RID(-1)
					value: arg
					typ:   arg.typ
				}
				return true, ref, RefLevel.pred_args
			}
		}

		visited[new_id] = true
		for pred in new.predecessors {
			to_visit.push(pred)
		}
	}

	// check if its in our function's arguments
	for arg in fun.args {
		if arg.name == name {
			ref := IRRef{
				id:    RID(-1)
				value: arg
			}
			return true, ref, RefLevel.fun_args
		}
	}

	// check if its in our owning namespace
	if name in ns.variable_map {
		ref := IRRef{
			id:    RID(-1)
			value: IRRefSum(ns.variable_map[name])
		}
		return true, ref, RefLevel.namespace
	}

	return false, IRRef{}, RefLevel.invalid
}

pub fn (mut b IRBuilder) bb_add_reference(bbid BBID, vid VID) RID {
	ref := IRRef{
		id:    b.functions[b.basic_blocks[bbid].function].refs.len
		value: IRRefSum(vid)
		typ:   b.variables[vid].typ
	}
	b.functions[b.basic_blocks[bbid].function].refs << ref
	b.basic_blocks[bbid].vars_map[b.variables[vid].name] = ref.id
	return ref.id
}

pub fn (mut b IRBuilder) bb_add_anon_reference(bbid BBID, iid IID, typ IRType) RID {
	ref := IRRef{
		id:    b.functions[b.basic_blocks[bbid].function].refs.len
		value: IRRefSum(iid)
		typ:   typ
	}
	b.functions[b.basic_blocks[bbid].function].refs << ref
	return ref.id
}

// pub fn (mut b IRBuilder) fill_bb_insts_expr(bbid BBID, fid FID, expr Expr) {
// 	bb := b.basic_blocks[bbid]
// 	func := b.function[fid]
// }
pub fn (mut b IRBuilder) bb_add_value_ir(bbid BBID, name string, typ IRType, source ValueType) VID {
	bb := b.basic_blocks[bbid]
	fid := bb.function
	func := b.functions[fid]
	ns := b.namespaces[func.namespace]
	mut val := IRValue{
		name:     name
		id:       b.variables.len
		typ:      typ
		storage:  source.to_ir()
		location: match source {
			.effemeral {
				IRLocation(IREffLocation{
					value: name
				})
			}
			.data {
				IRLocation(IRDataLocation{
					namespace: ns.id
					function:  fid
				})
			}
			.register {
				IRLocation(IRRegLocation{
					namespace: ns.id
					function:  fid
				})
			}
		}
	}
	b.variables << val
	return val.id
}

pub fn (mut b IRBuilder) bb_add_value_ast(bbid BBID, name string, typ Type, source ValueType) VID {
	bb := b.basic_blocks[bbid]
	fid := bb.function
	func := b.functions[fid]
	ns := b.namespaces[func.namespace]
	ir_typ := b.namespace_yeild_ir_type(ns, typ) or {
		if b.errors != unsafe { nil } {
			b.errors.error(.cannot_resolve_type, empty_location(), "could not resolve type for '${name}'")
		}
		return VID(-1)
	}
	mut val := IRValue{
		name:    name
		id:      b.variables.len
		typ:     ir_typ
		storage: source.to_ir()
		location: match source {
			.effemeral {
				IRLocation(IREffLocation{
					value: name
				})
			}
			.data {
				IRLocation(IRDataLocation{
					namespace: ns.id
					function:  fid
				})
			}
			.register {
				IRLocation(IRRegLocation{
					namespace: ns.id
					function:  fid
				})
			}
		}
	}
	b.variables << val
	return val.id
}

// Handle macro expressions with full identifier chain support including nesting.
// Returns (is_referable, RID) - the reference ID can be used to access the macro value.
// Each element in the chain is processed, creating intermediate instructions as needed.
pub fn (mut b IRBuilder) bb_handle_macro_expr(bbid BBID, mexpr MacroExpr) (bool, RID) {
	refed := mexpr.referable
	fid := b.basic_blocks[bbid].function
	chain := mexpr.ident_chain

	if chain.elements.len == 0 {
		if b.errors != unsafe { nil } {
			b.errors.error(.empty_macro_chain, empty_location(), 'empty macro expression chain')
		}
		return false, RID(-1)
	}

	// Process the first element to get the base
	mut current_rid := RID(-1)
	mut current_type := IRType(BuiltinType.void_t)

	first := chain.elements[0]
	match first {
		IdentifierChainName {
			ok, ref := b.bb_get_reference(first.name.name, bbid, fid)
			if !ok {
				if b.errors != unsafe { nil } {
					b.errors.error_with_hint(.undefined_variable, SourceLocation{
						file: ''
						pos:  first.name.pos
						len:  first.name.name.len
					}, "undefined variable '${first.name.name}' in macro expression", 'ensure the variable is defined before using it in a macro')
				}
				return false, RID(-1)
			}

			// Check if already a BB argument
			match ref.value {
				IRBasicBlockArg {
					if ref.value.id == bbid {
						current_rid = ref.id
						current_type = ref.typ
					} else {
						// Add as BB argument
						current_rid = b.bb_add_macro_arg(bbid, fid, first.name.name, ref)
						current_type = ref.typ
					}
				}
				else {
					// Add as BB argument
					current_rid = b.bb_add_macro_arg(bbid, fid, first.name.name, ref)
					current_type = ref.typ
				}
			}
		}
		IdentifierChainMacro {
			// Nested macro at start - recursively handle
			_, nested_rid := b.bb_handle_macro_expr(bbid, first.macro_expr)
			current_rid = nested_rid
			current_type = b.functions[fid].refs[nested_rid].typ
		}
		IdentifierChainIndex {
			// Index at start doesn't make sense without a base
			if b.errors != unsafe { nil } {
				b.errors.error(.invalid_operation, empty_location(), 'cannot start macro chain with index access')
			}
			return false, RID(-1)
		}
		IdentifierChainDeref {
			// Deref at start doesn't make sense without a base
			if b.errors != unsafe { nil } {
				b.errors.error(.invalid_operation, empty_location(), 'cannot start macro chain with dereference')
			}
			return false, RID(-1)
		}
	}

	// Process remaining chain elements
	for i in 1 .. chain.elements.len {
		elem := chain.elements[i]
		match elem {
			IdentifierChainName {
				// Field access - create IRFieldAccess
				mut inst := IRFieldAccess{
					id:     b.functions[fid].insts.len
					source: current_rid
					field:  elem.name.name
				}
				// Look up field type from struct definition if possible
				field_type := b.get_field_type(current_type, elem.name.name) or { current_type }
				inst.result = b.bb_add_anon_reference(bbid, inst.id, field_type)
				b.functions[fid].insts << inst
				b.basic_blocks[bbid].insts << inst.id
				current_rid = inst.result
				current_type = field_type
			}
			IdentifierChainMacro {
				// Nested macro for dynamic field access - create index access with macro value
				_, nested_rid := b.bb_handle_macro_expr(bbid, elem.macro_expr)
				mut inst := IRIndexAccess{
					id:       b.functions[fid].insts.len
					source:   current_rid
					index:    OID(nested_rid)
					is_slice: false
				}
				inst.result = b.bb_add_anon_reference(bbid, inst.id, current_type)
				b.functions[fid].insts << inst
				b.basic_blocks[bbid].insts << inst.id
				current_rid = inst.result
			}
			IdentifierChainIndex {
				// Array/list index - create IRIndexAccess
				index_oid := b.lower_expression(bbid, fid, elem.index_expr)
				mut inst := IRIndexAccess{
					id:       b.functions[fid].insts.len
					source:   current_rid
					index:    index_oid
					is_slice: false
				}
				inst.result = b.bb_add_anon_reference(bbid, inst.id, current_type)
				b.functions[fid].insts << inst
				b.basic_blocks[bbid].insts << inst.id
				current_rid = inst.result
			}
			IdentifierChainDeref {
				// Dereference - create IRDeref
				mut inst := IRDeref{
					id:     b.functions[fid].insts.len
					source: current_rid
				}
				// Result type is the dereferenced type
				mut result_type := current_type
				if current_type is IRRefType {
					ref_type := current_type as IRRefType
					result_type = ref_type.base
				}
				inst.result = b.bb_add_anon_reference(bbid, inst.id, result_type)
				b.functions[fid].insts << inst
				b.basic_blocks[bbid].insts << inst.id
				current_rid = inst.result
				current_type = result_type
			}
		}
	}

	return refed, current_rid
}

// Helper to add a macro argument to a basic block
fn (mut b IRBuilder) bb_add_macro_arg(bbid BBID, fid FID, name string, ref IRRef) RID {
	mut arg := IRBasicBlockArg{
		id:      bbid
		typ:     ref.typ
		storage: .data
		name:    name
	}
	b.basic_blocks[bbid].args << arg
	return ref.id
}

// Helper to get field type from a struct type
fn (mut b IRBuilder) get_field_type(typ IRType, field_name string) ?IRType {
	match typ {
		SID {
			struct_def := b.structs[typ]
			return struct_def.fields[field_name] or { return none }
		}
		IRRefType {
			return b.get_field_type(typ.base, field_name)
		}
		else {
			return none
		}
	}
}

// Returns an oid that we can set to something, later on we can merge the instructions together
pub fn (mut b IRBuilder) lower_expression(bbid BBID, fid FID, expr Expr) OID {
	bb := b.basic_blocks[bbid]
	func := b.functions[fid]
	ns := b.namespaces[func.namespace]
	match expr {
		BinaryExpr {
			mut inst := IRBinaryOp{
				op:    expr.operator.to_binary_op()
				left:  b.lower_expression(bbid, fid, expr.left)
				right: b.lower_expression(bbid, fid, expr.right)
			}
			inst.id = b.functions[fid].insts.len
			inst.result = b.bb_add_anon_reference(bbid, inst.id, inst.left.to_ir_type(func))
			b.functions[fid].insts << inst
			b.basic_blocks[bbid].insts << inst.id
			return OID(inst.result)
		}
		UnaryExpr {
			unary_op := expr.operator.to_unary_op()
			operand_oid := b.lower_expression(bbid, fid, expr.right)

			match unary_op {
				.ref {
					// Reference operator (&) - creates a pointer to the operand
					operand_rid := match operand_oid {
						RID { operand_oid }
						CID { panic('Cannot take reference of constant') }
					}
					mut inst := IRRefInst{
						id:     b.functions[fid].insts.len
						source: operand_rid
					}
					// Result type is a reference to the operand type
					base_type := operand_oid.to_ir_type(func)
					inst.result = b.bb_add_anon_reference(bbid, inst.id, IRType(IRRefType{ base: base_type }))
					b.functions[fid].insts << inst
					b.basic_blocks[bbid].insts << inst.id
					return OID(inst.result)
				}
				.deref {
					// Dereference operator (@) - gets the value pointed to
					operand_rid := match operand_oid {
						RID { operand_oid }
						CID { panic('Cannot dereference constant') }
					}
					mut inst := IRDeref{
						id:     b.functions[fid].insts.len
						source: operand_rid
					}
					// Result type is the dereferenced type
					base_type := operand_oid.to_ir_type(func)
					result_type := match base_type {
						IRRefType { base_type.base }
						else { base_type } // Allow deref on non-ref for flexibility
					}
					inst.result = b.bb_add_anon_reference(bbid, inst.id, result_type)
					b.functions[fid].insts << inst
					b.basic_blocks[bbid].insts << inst.id
					return OID(inst.result)
				}
				.neg {
					// Negation - use regular unary op
					mut inst := IRUnaryOp{
						op:      unary_op
						operand: operand_oid
					}
					inst.id = b.functions[fid].insts.len
					inst.result = b.bb_add_anon_reference(bbid, inst.id, operand_oid.to_ir_type(func))
					b.functions[fid].insts << inst
					b.basic_blocks[bbid].insts << inst.id
					return OID(inst.result)
				}
			}
		}
		Literal {
			lit := expr
			match lit {
				IntegerLiteral {
					cid := CID(b.functions[fid].consts.len)
					b.functions[fid].consts << IRIntConst{
						value: lit.value
					}
					return OID(cid)
				}
				CharLiteral {
					cid := CID(b.functions[fid].consts.len)
					b.functions[fid].consts << IRCharConst{
						value: lit.value
					}
					return OID(cid)
				}
				FloatLiteral {
					cid := CID(b.functions[fid].consts.len)
					b.functions[fid].consts << IRFloatConst{
						value: lit.value
					}
					return OID(cid)
				}
				StringLiteral {
					mut ir_str := IRStringConst{}
					if lit.value != '' {
						ir_str.parts << IRStringText{
							text: lit.value
						}
					}
					if lit.interpolated {
						for part in lit.parts {
							if !part.is_macro {
								ir_str.parts << IRStringText{
									text: part.text
								}
								continue
							}
							// We have a macro that we have to handle here...
							refed, mrid := b.bb_handle_macro_expr(bbid, part.macro_expr or {
								panic('should be a macro expression in this string literal???')
							})
							mcro := IRStringMacro{
								value:  mrid
								is_ref: refed
							}
							ir_str.parts << mcro
						}
					}
					cid := CID(b.functions[fid].consts.len)
					b.functions[fid].consts << ir_str
					return OID(cid)
				}
				ListLiteral {
					mut list_const := IRListConst{}
					for elem in lit.elements {
						list_const.elements << b.lower_expression(bbid, fid, elem)
					}
					cid := CID(b.functions[fid].consts.len)
					b.functions[fid].consts << list_const
					return OID(cid)
				}
				DictionaryLiteral {
					mut dict_const := IRDictConst{}
					for entry in lit.entries {
						mut key_oid := OID(CID(-1))
						match entry.key_kind {
							.integer_key {
								key_cid := CID(b.functions[fid].consts.len)
								b.functions[fid].consts << IRIntConst{
									value: entry.integer_key or { 0 }
								}
								key_oid = OID(key_cid)
							}
							.string_key {
								str_key := entry.string_key or { StringLiteral{} }
								key_cid := CID(b.functions[fid].consts.len)
								b.functions[fid].consts << IRStringConst{
									parts: [IRStringPart(IRStringText{ text: str_key.value })]
								}
								key_oid = OID(key_cid)
							}
							.macro_key {
								macro_key := entry.macro_key or { panic('expected macro key') }
								_, mrid := b.bb_handle_macro_expr(bbid, macro_key)
								key_oid = OID(mrid)
							}
						}
						dict_const.entries << IRDictEntry{
							key:   key_oid
							value: b.lower_expression(bbid, fid, entry.value)
						}
					}
					cid := CID(b.functions[fid].consts.len)
					b.functions[fid].consts << dict_const
					return OID(cid)
				}
				RangeLiteral {
					mut range_const := IRRangeConst{}
					if start := lit.start {
						range_const.start = b.lower_expression(bbid, fid, start)
					}
					if end := lit.end {
						range_const.end = b.lower_expression(bbid, fid, end)
					}
					cid := CID(b.functions[fid].consts.len)
					b.functions[fid].consts << range_const
					return OID(cid)
				}
			}
		}
		Identifier {
			ok, ref := b.bb_get_reference(expr.name, bbid, fid)
			if ok {
				return OID(ref.id)
			} else {
				if b.errors != unsafe { nil } {
					b.errors.error(.undefined_variable, SourceLocation{
						pos: expr.pos
						len: expr.name.len
					}, "undefined variable '${expr.name}'")
				}
				return OID(CID(-1))
			}
		}
		MacroExpr {
			// Macro expressions become basic block arguments
			_, mrid := b.bb_handle_macro_expr(bbid, expr)
			return OID(mrid)
		}
		AccessExpr {
			access := expr
			match access {
				MemberAccessExpr {
					// Lower the target first
					mut current_oid := b.lower_expression(bbid, fid, access.target)

					// Process the chain
					for elem in access.chain {
						match elem {
							FieldAccessElement {
								// Field access: create IRFieldAccess instruction
								if current_oid is CID {
									panic('Cannot access field on constant')
								}
								current_rid := current_oid as RID
								mut inst := IRFieldAccess{
									id:     b.functions[fid].insts.len
									source: current_rid
									field:  elem.field.name
								}
								// Result type needs to be looked up from struct definition
								inst.result = b.bb_add_anon_reference(bbid, inst.id, current_oid.to_ir_type(b.functions[fid]))
								b.functions[fid].insts << inst
								b.basic_blocks[bbid].insts << inst.id
								current_oid = OID(inst.result)
							}
							IndexAccessElement {
								// Index access: create IRIndexAccess instruction
								if current_oid is CID {
									panic('Cannot index into constant')
								}
								current_rid := current_oid as RID
								mut inst := IRIndexAccess{
									id:       b.functions[fid].insts.len
									source:   current_rid
									index:    b.lower_expression(bbid, fid, elem.index_expr)
									is_slice: elem.is_slice
								}
								if elem.is_slice {
									if end := elem.slice_end {
										inst.end = b.lower_expression(bbid, fid, end)
									}
								}
								inst.result = b.bb_add_anon_reference(bbid, inst.id, current_oid.to_ir_type(b.functions[fid]))
								b.functions[fid].insts << inst
								b.basic_blocks[bbid].insts << inst.id
								current_oid = OID(inst.result)
							}
							MacroAccessElement {
								// Macro in access chain - need to handle this specially
								_, mrid := b.bb_handle_macro_expr(bbid, elem.macro_expr)
								if current_oid is CID {
									panic('Cannot access macro field on constant')
								}
								current_rid := current_oid as RID
								mut inst := IRIndexAccess{
									id:       b.functions[fid].insts.len
									source:   current_rid
									index:    OID(mrid)
									is_slice: false
								}
								inst.result = b.bb_add_anon_reference(bbid, inst.id, current_oid.to_ir_type(b.functions[fid]))
								b.functions[fid].insts << inst
								b.basic_blocks[bbid].insts << inst.id
								current_oid = OID(inst.result)
							}
							DerefAccessElement {
								// Dereference: create IRDeref instruction
								if current_oid is CID {
									panic('Cannot dereference constant')
								}
								current_rid := current_oid as RID
								mut inst := IRDeref{
									id:     b.functions[fid].insts.len
									source: current_rid
								}
								// Result type is the dereferenced type
								base_type := current_oid.to_ir_type(b.functions[fid])
								mut result_type := base_type
								if base_type is IRRefType {
									ref_type := base_type as IRRefType
									result_type = ref_type.base
								}
								inst.result = b.bb_add_anon_reference(bbid, inst.id, result_type)
								b.functions[fid].insts << inst
								b.basic_blocks[bbid].insts << inst.id
								current_oid = OID(inst.result)
							}
						}
					}
					return current_oid
				}
				IndexAccessExpr {
					// Direct index access
					target_oid := b.lower_expression(bbid, fid, access.target)
					target_rid := match target_oid {
						RID { target_oid }
						CID { panic('Cannot index into constant directly') }
					}
					mut inst := IRIndexAccess{
						id:       b.functions[fid].insts.len
						source:   target_rid
						index:    b.lower_expression(bbid, fid, access.index.index_expr)
						is_slice: access.index.is_slice
					}
					if access.index.is_slice {
						if end := access.index.slice_end {
							inst.end = b.lower_expression(bbid, fid, end)
						}
					}
					inst.result = b.bb_add_anon_reference(bbid, inst.id, target_oid.to_ir_type(b.functions[fid]))
					b.functions[fid].insts << inst
					b.basic_blocks[bbid].insts << inst.id
					return OID(inst.result)
				}
				FunctionCallExpr {
					// Function calls - need to resolve the function and create IRCall
					mut call_fid := FID(-1)

					// Resolve the function being called
					base := access.base_target
					match base {
						Identifier {
							// Look up in current namespace
							call_fid = b.namespaces[ns.id].function_map[base.name] or {
								if b.errors != unsafe { nil } {
									b.errors.error(.undefined_function, SourceLocation{
										pos: base.pos
										len: base.name.len
									}, "undefined function '${base.name}'")
								}
								return OID(CID(-1))
							}
						}
						QualifiedIdentifier {
							// Traverse namespace path
							ns_path := base.to_list()
							target_nid := b.tranverse_namespace(ns, ns_path) or {
								if b.errors != unsafe { nil } {
									b.errors.error(.undefined_namespace, SourceLocation{
										pos: base.pos
									}, "could not resolve namespace path for function '${base.name.name}'")
								}
								return OID(CID(-1))
							}
							call_fid = b.namespaces[target_nid].function_map[base.name.name] or {
								if b.errors != unsafe { nil } {
									b.errors.error(.undefined_function, SourceLocation{
										pos: base.pos
									}, "function '${base.name.name}' not found in namespace")
								}
								return OID(CID(-1))
							}
						}
						else {
							if b.errors != unsafe { nil } {
								b.errors.error(.invalid_operation, empty_location(), 'cannot call non-identifier expression as function')
							}
							return OID(CID(-1))
						}
					}

					// Lower arguments
					mut arg_oids := []OID{}
					for arg in access.args {
						arg_oids << b.lower_expression(bbid, fid, arg)
					}

					// Create IRCall instruction
					mut inst := IRCall{
						id:       b.functions[fid].insts.len
						function: call_fid
						args:     arg_oids
					}

					// Set result if function returns non-void
					called_func := b.functions[call_fid]
					if called_func.return_type != IRType(BuiltinType.void_t) {
						inst.result = b.bb_add_anon_reference(bbid, inst.id, called_func.return_type)
					}

					b.functions[fid].insts << inst
					b.basic_blocks[bbid].insts << inst.id

					if result_rid := inst.result {
						return OID(result_rid)
					}
					// Void return - return a dummy
					return OID(CID(-1))
				}
			}
		}
		StructLiteral {
			// Look up the struct type
			ns_path := expr.struct_name.to_list()
			target_nid := b.tranverse_namespace(ns, ns_path) or {
				if b.errors != unsafe { nil } {
					b.errors.error(.undefined_namespace, SourceLocation{
						pos: expr.struct_name.pos
					}, "could not resolve namespace path for struct '${expr.struct_name.name.name}'")
				}
				return OID(CID(-1))
			}
			struct_id := b.namespaces[target_nid].struct_map[expr.struct_name.name.name] or {
				if b.errors != unsafe { nil } {
					b.errors.error(.undefined_struct, SourceLocation{
						pos: expr.struct_name.pos
					}, "struct '${expr.struct_name.name.name}' not found")
				}
				return OID(CID(-1))
			}

			// Create struct init instruction
			mut inst := IRStructInit{
				id:          b.functions[fid].insts.len
				struct_type: struct_id
			}

			// Lower field values
			for field in expr.fields {
				inst.field_values[field.name.name] = b.lower_expression(bbid, fid, field.value)
			}

			inst.result = b.bb_add_anon_reference(bbid, inst.id, IRType(struct_id))
			b.functions[fid].insts << inst
			b.basic_blocks[bbid].insts << inst.id
			return OID(inst.result)
		}
		QualifiedIdentifier {
			// Resolve the qualified identifier to a variable
			ns_path := expr.to_list()
			target_nid := b.tranverse_namespace(ns, ns_path) or {
				if b.errors != unsafe { nil } {
					b.errors.error(.undefined_namespace, SourceLocation{
						pos: expr.pos
					}, "could not resolve namespace path for '${expr.name.name}'")
				}
				return OID(CID(-1))
			}
			// Look up variable in target namespace
			vid := b.namespaces[target_nid].variable_map[expr.name.name] or {
				if b.errors != unsafe { nil } {
					b.errors.error(.undefined_variable, SourceLocation{
						pos: expr.pos
					}, "variable '${expr.name.name}' not found in namespace")
				}
				return OID(CID(-1))
			}
			rid := b.bb_add_reference(bbid, vid)
			return OID(rid)
		}
	}

	if b.errors != unsafe { nil } {
		b.errors.error(.not_implemented, empty_location(), 'could not lower expression')
	}
	return OID(CID(-1))
}

pub fn (mut b IRBuilder) fill_bb_insts_stmt(bbid BBID, fid FID, stmt Stmt) {
	mut bb := b.basic_blocks[bbid]
	mut func := b.functions[fid]
	mut ns := b.namespaces[func.namespace]
	match stmt {
		TypedDefine {
			ok, ref := b.bb_get_reference(stmt.name.name, bbid, fid)
			if ok { // We have found a reference to this (Or made one)
				// This should not be allowed.... We are not allowed to have
				// this sort of shadowing!
				if b.errors != unsafe { nil } {
					b.errors.error(.duplicate_definition, SourceLocation{
						pos: stmt.Node.pos
					}, "variable '${stmt.name.name}' is already defined")
				}
				return
			} else { // No reference to this exists so we should like make one
				// need to create a value
				inst := IRTypedDefine{
					id:     func.insts.len
					result: b.bb_add_reference(bbid, b.bb_add_value_ast(bbid, stmt.name.name,
						stmt.typ, stmt.source))
				}
				b.functions[fid].insts << inst
				b.basic_blocks[bbid].insts << inst.id
			}
		}
		Define {
			ok, ref := b.bb_get_reference(stmt.name.name, bbid, fid)
			if ok { // We have found a reference to this (or have made one)
				// This is not allowed, this is a definition. We are not allowed to have shadowing.
				if b.errors != unsafe { nil } {
					b.errors.error(.shadowing_not_allowed, SourceLocation{
						pos: stmt.Node.pos
					}, "variable '${stmt.name.name}' would shadow an existing variable (shadowing is not allowed)")
				}
				return
			} else { // We are the first creation point for this thing!
				// We have to first figure out the type of the expression that we are coming from...
				// And generate the instructions that compose this expression...
				mut inst := IRDefine{
					value: b.lower_expression(bbid, fid, stmt.value)
				}
				inst.result = b.bb_add_reference(bbid, b.bb_add_value_ir(bbid, stmt.name.name,
					inst.value.to_ir_type(b.functions[fid]), stmt.source))
				inst.id = b.functions[fid].insts.len
				b.functions[fid].insts << inst
				b.basic_blocks[bbid].insts << inst.id
			}
		}
		Assignment {
			op := stmt.operator.to_assign_op()
			left_oid := b.lower_expression(bbid, fid, stmt.left)
			if left_oid is CID {
				if b.errors != unsafe { nil } {
					b.errors.error(.invalid_operation, SourceLocation{
						pos: stmt.Node.pos
					}, 'cannot assign to a constant value')
				}
				return
			}
			left := left_oid as RID

			right := b.lower_expression(bbid, fid, stmt.right)
			match op {
				.assign {
					// Check that left and right are same type.
					if OID(left).to_ir_type(b.functions[fid]) != right.to_ir_type(b.functions[fid]) {
						if b.errors != unsafe { nil } {
							b.errors.error(.type_mismatch, SourceLocation{
								pos: stmt.Node.pos
							}, 'cannot assign across different types')
						}
						return
					}
				}
				else {
					lref := b.functions[fid].refs[left]
					if lref.typ is BuiltinType {
						if lref.typ != BuiltinType.int_t {
							if b.errors != unsafe { nil } {
								b.errors.error(.invalid_operation, SourceLocation{
									pos: stmt.Node.pos
								}, 'cannot perform operation ${op} on type ${lref.typ}')
							}
							return
						}
					}
				}
			}
			mut inst := IRAssign{
				id:     b.functions[fid].insts.len
				result: left
				op:     op
				value:  right
			}
			b.functions[fid].insts << inst
			b.basic_blocks[bbid].insts << inst.id
		}
		Store {
			// Store is like assignment but for storage operations (<-)
			left_oid := b.lower_expression(bbid, fid, stmt.left)
			if left_oid is CID {
				if b.errors != unsafe { nil } {
					b.errors.error(.invalid_operation, SourceLocation{
						pos: stmt.Node.pos
					}, 'cannot store to a constant value')
				}
				return
			}
			left := left_oid as RID

			right_oid := b.lower_expression(bbid, fid, stmt.right)
			if right_oid is CID {
				if b.errors != unsafe { nil } {
					b.errors.error(.invalid_operation, SourceLocation{
						pos: stmt.Node.pos
					}, 'cannot store from a constant value')
				}
				return
			}
			right := right_oid as RID

			mut inst := IRStore{
				id:     b.functions[fid].insts.len
				result: left
				source: right
			}
			b.functions[fid].insts << inst
			b.basic_blocks[bbid].insts << inst.id
		}
		ExprStmt {
			// Expression statement - just evaluate the expression for side effects
			b.lower_expression(bbid, fid, stmt.expr)
		}
		MacroLiteralCommand {
			// $ command - lower to IRMacroLiteralCmd
			mut parts := []IRMacroCmdPart{}
			for part in stmt.parts {
				match part {
					MacroLiteralText {
						parts << IRMacroCmdPart(IRMacroCmdText{
							text: part.text
						})
					}
					MacroLiteralMacro {
						refed, mrid := b.bb_handle_macro_expr(bbid, part.macro_expr)
						parts << IRMacroCmdPart(IRMacroCmdMacro{
							value:  mrid
							is_ref: refed
						})
					}
					MacroLiteralString {
						mut str_parts := []IRStringPart{}
						str_lit := part.str_literal
						if !str_lit.interpolated {
							str_parts << IRStringPart(IRStringText{
								text: str_lit.value
							})
						} else {
							for str_part in str_lit.parts {
								if !str_part.is_macro {
									str_parts << IRStringPart(IRStringText{
										text: str_part.text
									})
								} else {
									macro_expr := str_part.macro_expr or { continue }
									refed, mrid := b.bb_handle_macro_expr(bbid, macro_expr)
									str_parts << IRStringPart(IRStringMacro{
										value:  mrid
										is_ref: refed
									})
								}
							}
						}
						parts << IRMacroCmdPart(IRMacroCmdString{
							parts: str_parts
						})
					}
				}
			}
			inst := IRMacroLiteralCmd{
				func:  fid
				id:    b.functions[fid].insts.len
				parts: parts
			}
			b.functions[fid].insts << inst
			b.basic_blocks[bbid].insts << inst.id
		}
		IfStmt {
			// Lower the condition expression - actual branching is handled by terminators
			b.lower_expression(bbid, fid, stmt.condition)
			// Note: The then/else blocks are handled as separate basic blocks
			// Terminators (IRBranch) are added in a later pass
		}
		Return {
			// Return statement - handled as terminator instruction
			if val := stmt.value {
				ret_val := b.lower_expression(bbid, fid, val)
				inst := IRReturn{
					id:    b.functions[fid].insts.len
					value: ret_val
				}
				b.functions[fid].insts << inst
				b.basic_blocks[bbid].insts << inst.id
			} else {
				inst := IRReturn{
					id:    b.functions[fid].insts.len
					value: none
				}
				b.functions[fid].insts << inst
				b.basic_blocks[bbid].insts << inst.id
			}
		}
		StructDefinition, FunctionDefinition, NamespaceDefinition,
		NamespaceImport, NamespaceAlias, FunctionInlineDefinition, Block {
			// These shouldn't appear in basic blocks
			if b.errors != unsafe { nil } {
				b.errors.error(.unsupported_statement_location, empty_location(), 'invalid statement in basic block: ${stmt}')
			}
		}
	}
}

pub fn (mut b IRBuilder) fill_bb_insts(bbid BBID) {
	bb := b.basic_blocks[bbid]
	fid := bb.function
	for stmt in bb.stmts {
		b.fill_bb_insts_stmt(bbid, fid, stmt)
	}
}

// Add terminator instructions to basic blocks based on CFG structure
pub fn (mut b IRBuilder) add_bb_terminator(bbid BBID) {
	bb := b.basic_blocks[bbid]
	fid := bb.function

	// Check if block already has a terminator (Return)
	if bb.insts.len > 0 {
		last_inst := b.functions[fid].insts[bb.insts[bb.insts.len - 1]]
		if last_inst is IRReturn {
			return // Already has terminator
		}
	}

	// Check what kind of terminator we need based on successors
	match bb.successors.len {
		0 {
			// No successors - this should have a return (implicit void return)
			inst := IRReturn{
				id:    b.functions[fid].insts.len
				value: none
			}
			b.functions[fid].insts << inst
			b.basic_blocks[bbid].insts << inst.id
		}
		1 {
			// One successor - unconditional jump
			target := bb.successors[0]
			// Collect arguments for the target block
			mut args := []OID{}
			for arg in b.basic_blocks[target].args {
				// Look up the value in current scope and pass it
				ok, ref := b.bb_get_reference(arg.name, bbid, fid)
				if ok {
					args << OID(ref.id)
				} else {
					if b.errors != unsafe { nil } {
						b.errors.error(.undefined_variable, empty_location(), "could not find argument '${arg.name}' to pass to block")
					}
					continue
				}
			}
			inst := IRJump{
				id:     b.functions[fid].insts.len
				target: target
				args:   args
			}
			b.functions[fid].insts << inst
			b.basic_blocks[bbid].insts << inst.id
		}
		2 {
			// Two successors - conditional branch (if/else)
			// The condition should be the last evaluated expression before branching
			// Look for the IfStmt in the block's statements
			mut cond_oid := OID(CID(-1))
			for stmt in bb.stmts {
				if stmt is IfStmt {
					cond_oid = b.lower_expression(bbid, fid, stmt.condition)
					break
				}
			}

			then_bb := bb.successors[0]
			else_bb := bb.successors[1]

			// Collect arguments for both branches
			mut then_args := []OID{}
			for arg in b.basic_blocks[then_bb].args {
				ok, ref := b.bb_get_reference(arg.name, bbid, fid)
				if ok {
					then_args << OID(ref.id)
				}
			}

			mut else_args := []OID{}
			for arg in b.basic_blocks[else_bb].args {
				ok, ref := b.bb_get_reference(arg.name, bbid, fid)
				if ok {
					else_args << OID(ref.id)
				}
			}

			inst := IRBranch{
				id:        b.functions[fid].insts.len
				cond:      cond_oid
				then_bb:   then_bb
				then_args: then_args
				else_bb:   else_bb
				else_args: else_args
			}
			b.functions[fid].insts << inst
			b.basic_blocks[bbid].insts << inst.id
		}
		else {
			if b.errors != unsafe { nil } {
				b.errors.error(.internal_compiler_error, empty_location(), 'basic block has more than 2 successors, which is not supported')
			}
		}
	}
}

// Add terminators to all basic blocks in a function
pub fn (mut b IRBuilder) add_function_terminators(fid FID) {
	for bbid in b.functions[fid].bbs {
		b.add_bb_terminator(bbid)
	}
}

// Fill all instructions for all basic blocks in a function
pub fn (mut b IRBuilder) fill_function_insts(fid FID) {
	for bbid in b.functions[fid].bbs {
		b.fill_bb_insts(bbid)
	}
	// Add terminators after all instructions are filled
	b.add_function_terminators(fid)
}

// Solve basic block arguments - determine what values need to be passed between blocks
pub fn (mut b IRBuilder) solve_bb_arguments(fid FID) {
	// For each basic block, check what variables it uses that are defined elsewhere
	for bbid in b.functions[fid].bbs {
		bb := b.basic_blocks[bbid]

		// Skip entry block - it gets arguments from function arguments
		if bbid == b.functions[fid].entrybb {
			continue
		}

		// For blocks that need macro values, the args are already added by bb_handle_macro_expr
		// This function ensures consistency and handles phi-like scenarios

		// Check each predecessor and ensure they can provide the required arguments
		for pred_id in bb.predecessors {
			pred := b.basic_blocks[pred_id]
			for arg in bb.args {
				ok, _, level := b.bb_get_reference_no_creation(arg.name, pred_id, fid)
				if !ok {
					// The predecessor doesn't have this value - it may need to get it from further up
					// This is handled by the basic block argument passing in terminators
				}
			}
		}
	}
}
