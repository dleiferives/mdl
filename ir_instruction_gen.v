module main

import datatypes

// gets it and ads it to the local map
pub fn (mut b IRBuilder) bb_get_reference(name string, bbid BBID, fid FID) (bool, IRRef) {
	mut bb := b.basic_blocks[bbid]
	mut fun := b.functions[fid]
	mut ns := b.namespaces[fun.namespace]

	// check if its in our arguments
	if name in bb.vars_map {
		return true, fun.refs[bb.vars_map[name]]
	}
	for arg in bb.args {
		if arg.name == name {
			ref := IRRef{
				id:    fun.refs.len
				value: arg
				typ:   arg.typ
			}
			fun.refs << ref
			bb.vars_map[name] = ref.id
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
		new_id := to_visit.pop() or { panic("we really thought we'd have something lol") }
		if new_id in visited {
			continue
		}
		new := b.basic_blocks[new_id]
		if name in new.vars_map {
			ref := new.vars_map[name]
			b.basic_blocks[bbid].vars_map[name] = ref
			return true, fun.refs[ref]
		}
		visited[new_id] = true
		for pred in new.predecessors {
			to_visit.push(pred)
		}
	}

	// check if its in our owning namespace
	if name in ns.variable_map {
		ref := IRRef{
			id:    fun.refs.len
			value: IRRefSum(ns.variable_map[name])
		}
		fun.refs << ref
		b.basic_blocks[bbid].vars_map[name] = ref.id
		return true, ref
	}

	// check if its in our function's arguments
	for arg in fun.args {
		if arg.name == name {
			ref := IRRef{
				id:    fun.refs.len
				value: arg
			}
			fun.refs << ref
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
		new_id := to_visit.pop() or { panic("we really thought we'd have something lol") }
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
	mut val := IRValue{
		name:     name
		id:       b.variables.len
		typ:      b.namespace_yeild_ir_type(ns, typ) or {
			panic('could not find type for typed define')
		}
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

// returns if its a refered macro, and the RID for accessing it!
pub fn (mut b IRBuilder) bb_handle_macro_expr(bbid BBID, mexpr MacroExpr) (bool, RID) {
	refed := mexpr.referable
	fid := b.basic_blocks[bbid].function
	// TODO: Implement going down the identifier chain -> will have to make macro expasions and such....
	// which will create more basic blocks.
	chain := mexpr.ident_chain
	if chain.elements.len > 1 {
		panic('Not implemented identifier chains yet...')
	}
	e := chain.elements[0]
	match e {
		IdentifierChainName {
			ok, ref := b.bb_get_reference(e.name.name, bbid, fid)
			if !ok {
				panic('could not find value to macro expand in ${mexpr}')
			}

			match ref.value {
				IRBasicBlockArg {
					if ref.value.id == bbid {
						// we don't have to add it as something that we want, someone has already done this for us.
						return refed, ref.id
					}
				}
				else {}
			}

			// It is not an argument to ourself... We need to ask for this thing.
			// As we are going to be using it as a macro value it has to be as a data -> we can set the storage type to data
			// type IRRefSum = VID | IRBasicBlockArg | IRFunctionArg | IID
			mut arg := IRBasicBlockArg{
				id:      bbid
				typ:     ref.typ
				storage: .data
			}
			match ref.value {
				IRBasicBlockArg {
					arg.name = ref.value.name
				}
				IRFunctionArg {
					arg.name = ref.value.name
				}
				VID {
					arg.name = b.variables[ref.value].name
				}
				IID {
					panic('should be impossible for for an instruction to be the reference for a name...')
					arg.name = ''
				}
			}
			b.basic_blocks[bbid].args << arg
			return refed, ref.id
		}
		else {
			panic('not implemented anything other than identiferi chain name for macroexpr ${e}')
		}
	}

	panic('could not handle macro expr')
	return refed, RID(-1)
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
			// Will become not a literal, therefore we can have it not be a constant anymore
			mut inst := IRUnaryOp{
				op:      expr.operator.to_unary_op()
				operand: b.lower_expression(bbid, fid, expr.right)
			}
			inst.id = b.functions[fid].insts.len
			inst.result = b.bb_add_anon_reference(bbid, inst.id, inst.operand.to_ir_type(func))
			b.functions[fid].insts << inst
			b.basic_blocks[bbid].insts << inst.id
			return OID(inst.result)
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
					panic('list literal not implemented')
				}
				DictionaryLiteral {
					panic('dictionary literal not implemented')
				}
				RangeLiteral {
					panic('range literal not implemented')
				}
			}
		}
		Identifier { // Will have to be a ref, and it must exist! otherwise we're boned boys
			ok, ref := b.bb_get_reference(expr.name, bbid, fid)
			if ok {
				return OID(ref.id)
			} else {
				panic('We could not find a reference to the identifier ${expr}')
			}
		}
		MacroExpr {
			panic('not implemented macro expr yet for lowering expressoin')
		}
		AccessExpr {
			panic('not implemente d access expressoin yet for loweing expression')
		}
		StructLiteral {
			panic('not implemented struct literal expr yet for loweing expressions')
		}
		QualifiedIdentifier {
			panic('not yet implemented qualified identifier  lowering for loweriing expresions yet')
		}
	}

	panic('could not lower expression')
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
				panic('When defining ${stmt} we found a reference to it already existing.')
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
				panic('When defining ${stmt} it would be shadowing something already in its scope. This is not allowed')
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
			left := match left_oid {
				CID {
					panic('Cannot assign to a constant value')
					RID(-1)
				}
				RID {
					left_oid
				}
			}

			right := b.lower_expression(bbid, fid, stmt.right)
			match op {
				.assign {
					// Check that left and right are same type.
					if OID(left).to_ir_type(b.functions[fid]) != right.to_ir_type(b.functions[fid]) {
						panic('cannot assign accross types')
					}
					// TODO: handle checking if the sources are different to
				}
				else {
					lref := b.functions[fid].refs[left]
					if lref.typ is BuiltinType {
						if lref.typ != BuiltinType.int_t {
							panic('cannot do operation ${op} onto type ${lref.typ}')
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
		else {
			println('not implemented ${stmt}')
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
