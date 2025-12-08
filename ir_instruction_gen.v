module main

import datatypes

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

pub fn (mut b IRBuilder) bb_add_reference(bbid BBID, vid VID) RID {
	ref := IRRef{
		id:    b.functions[b.basic_blocks[bbid].function].refs.len
		value: IRRefSum(vid)
	}
	b.functions[b.basic_blocks[bbid].function].refs << ref
	b.basic_blocks[bbid].vars_map[b.variables[vid].name] = ref.id
	return ref.id
}

// pub fn (mut b IRBuilder) fill_bb_insts_expr(bbid BBID, fid FID, expr Expr) {
// 	bb := b.basic_blocks[bbid]
// 	func := b.function[fid]
// }

pub fn (mut b IRBuilder) bb_add_value(bbid BBID, name string, typ Type, source ValueType) VID {
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

pub fn (mut b IRBuilder) fill_bb_insts_stmt(bbid BBID, fid FID, stmt Stmt) {
	// bb := b.basic_blocks[bbid]
	func := b.functions[fid]
	// ns := b.namespaces[func.namespace]
	match stmt {
		TypedDefine {
			ok, ref := b.bb_get_reference(stmt.name.name, bbid, fid)
			if ok { // We have found a reference to this (Or made one)
				inst := IRTypedDefine{
					id:     func.insts.len
					result: ref.id
				}
				b.functions[fid].insts << inst
				b.basic_blocks[bbid].insts << inst.id
			} else { // No reference to this exists so we should like make one
				// need to create a value
				inst := IRTypedDefine{
					id:     func.insts.len
					result: b.bb_add_reference(bbid, b.bb_add_value(bbid, stmt.name.name,
						stmt.typ, stmt.source))
				}
				b.functions[fid].insts << inst
				b.basic_blocks[bbid].insts << inst.id
			}
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
