module main

// CFG
pub fn (mut b IRBuilder) print_cfg_dot() {
	println('digraph G {')
	b.print_all_ns_dot(1)
	println('}')
}

pub fn (mut b IRBuilder) print_all_ns_dot(depth int) {
	for ns in b.namespaces {
		for _ in 0 .. depth {
			print('\t')
		}
		println('subgraph cluster_${ns.name}_${ns.id} {')
		for _ in 0 .. depth + 1 {
			print('\t')
		}
		println("label = \"${ns.name}\"")
		b.print_ns_all_fns_dot(ns.id, depth + 1)
		for _ in 0 .. depth {
			print('\t')
		}
		println('}')
	}
}

pub fn (mut b IRBuilder) print_ns_all_fns_dot(ns NID, depth int) {
	for fid in b.namespaces[ns].functions {
		func := b.functions[fid]
		for _ in 0 .. depth {
			print('\t')
		}
		println('subgraph cluster_${func.name}_${func.id} {')
		for _ in 0 .. depth + 1 {
			print('\t')
		}
		println("label = \"${func.name}\"")
		b.fn_print_bbs_graph_dot(func.id, depth + 1)
		for _ in 0 .. depth {
			print('\t')
		}
		println('}')
	}
}

pub fn (mut b IRBuilder) print_all_fns_dot(depth int) {
	for func in b.functions {
		for _ in 0 .. depth {
			print('\t')
		}
		println('subgraph cluster_${func.name} {')
		for _ in 0 .. depth + 1 {
			print('\t')
		}
		println("label = \"${func.name}\"")
		b.fn_print_bbs_graph_dot(func.id, depth + 1)
		for _ in 0 .. depth {
			print('\t')
		}
		println('}')
	}
}

pub fn (mut b IRBuilder) fn_print_bbs_graph_dot(fid FID, depth int) {
	for bbid in b.functions[fid].bbs {
		bb := b.basic_blocks[bbid]
		for _ in 0 .. depth {
			print('\t')
		}
		println('${bb.label}_${bb.id};')
	}

	for bbid in b.functions[fid].bbs {
		bb := b.basic_blocks[bbid]
		for suc_id in bb.successors {
			suc_name := b.basic_blocks[suc_id].label
			for _ in 0 .. depth {
				print('\t')
			}
			println('${bb.label}_${bb.id} -> ${suc_name}_${suc_id};')
		}
		for pred_id in bb.predecessors {
			pred_name := b.basic_blocks[pred_id].label
			for _ in 0 .. depth {
				print('\t')
			}
			println('${pred_name}_${pred_id} -> ${bb.label}_${bb.id};')
		}
	}
}

//  REFS

pub fn (mut b IRBuilder) print_ir_location(location IRLocation) {
	match location {
		IRRegLocation {
			ns := b.namespaces[location.namespace]
			function := location.function or {
				print('${ns.name} example')
				return
			}

			fun := b.functions[function]
			print('${ns.name}_${fun.name} example')
		}
		IRDataLocation {
			ns := b.namespaces[location.namespace]
			function := location.function or {
				print('example:${ns.name}')
				return
			}

			fun := b.functions[function]
			print('example:${ns.name} ${fun.name}')
		}
		IREffLocation {
			print('effemeral value existing as ${location.value}')
		}
	}
}

// TODO
pub fn (mut b IRBuilder) fn_print_rid(fid FID, rid RID) {
	func := b.functions[fid]
	ref := func.refs[rid]
	print('(${ref.typ} ')
	v := ref.value
	match v {
		VID {
			b.print_ir_location(b.variables[v].location)
			print(').${b.variables[v].name} ')
		}
		IRBasicBlockArg {}
		IRFunctionArg {}
		IID {
			print(b.namespaces[func.namespace].name)
			print(':')
			print(func.name)
			print(':inst_${v}')
		}
	}
}

pub fn (mut b IRBuilder) fn_print_oid(fid FID, id OID) {
	func := b.functions[fid]
	match id {
		CID {
			c := b.functions[fid].consts[id]
			match c {
				IRFloatConst {
					print('${c.value}')
				}
				IRIntConst {
					print('${c.value}')
				}
				IRCharConst {
					print('${c.value}')
				}
				IRStringConst {
					for part in c.parts {
						match part {
							IRStringText {
								print(part.text)
							}
							IRStringMacro {
								if part.is_ref {
									print('&')
								}
								b.fn_print_rid(fid, part.value)
							}
						}
					}
				}
				else {
					print('const ${c} not yet supported')
				}
			}
		}
		RID {
			b.fn_print_rid(fid, id)
		}
	}
}

pub fn (mut b IRBuilder) fn_print_inst(fid FID, iid IID) {
	func := b.functions[fid]
	inst := func.insts[iid]
	match inst {
		IRTypedDefine {
			print('define ')
			b.fn_print_rid(fid, inst.result)
		}
		IRDefine {
			print('define ')
			b.fn_print_rid(fid, inst.result)
			print(' = ')
			b.fn_print_oid(fid, inst.value)
		}
		IRAssign {
			print('assign ')
			b.fn_print_rid(fid, inst.result)
			inst.op.print()
			b.fn_print_oid(fid, inst.value)
		}
		IRBinaryOp {
			print('binop ')
			b.fn_print_rid(fid, inst.result)
			print(' = ')
			b.fn_print_oid(fid, inst.left)
			print(inst.op)
			b.fn_print_oid(fid, inst.right)
		}
		IRUnaryOp {
			print('unop ')
			b.fn_print_rid(fid, inst.result)
			print(' = ')
			print(inst.op)
			print(' ')
			b.fn_print_oid(fid, inst.operand)
		}
		IRStore {
			print('store ')
			b.fn_print_rid(fid, inst.result)
			print(' <- ')
			b.fn_print_rid(fid, inst.source)
		}
		IRCall {
			print('call ')
			if result := inst.result {
				b.fn_print_rid(fid, result)
				print(' = ')
			}
			called_func := b.functions[inst.function]
			print('${called_func.name}(')
			for i, arg in inst.args {
				if i > 0 {
					print(', ')
				}
				b.fn_print_oid(fid, arg)
			}
			print(')')
		}
		IRStructInit {
			print('struct_init ')
			b.fn_print_rid(fid, inst.result)
			print(' = ${b.structs[inst.struct_type].name}{')
			mut first := true
			for field_name, field_val in inst.field_values {
				if !first {
					print(', ')
				}
				first = false
				print('${field_name}: ')
				b.fn_print_oid(fid, field_val)
			}
			print('}')
		}
		IRFieldAccess {
			print('field ')
			b.fn_print_rid(fid, inst.result)
			print(' = ')
			b.fn_print_rid(fid, inst.source)
			print('.${inst.field}')
		}
		IRIndexAccess {
			print('index ')
			b.fn_print_rid(fid, inst.result)
			print(' = ')
			b.fn_print_rid(fid, inst.source)
			print('[')
			b.fn_print_oid(fid, inst.index)
			if inst.is_slice {
				print('..')
				if end := inst.end {
					b.fn_print_oid(fid, end)
				}
			}
			print(']')
		}
		IRDeref {
			print('deref ')
			b.fn_print_rid(fid, inst.result)
			print(' = @')
			b.fn_print_rid(fid, inst.source)
		}
		IRRefInst {
			print('ref ')
			b.fn_print_rid(fid, inst.result)
			print(' = &')
			b.fn_print_rid(fid, inst.source)
		}
		IRJump {
			print('jump ')
			target := b.basic_blocks[inst.target]
			print('${target.label}_${target.id}')
			if inst.args.len > 0 {
				print('(')
				for i, arg in inst.args {
					if i > 0 {
						print(', ')
					}
					b.fn_print_oid(fid, arg)
				}
				print(')')
			}
		}
		IRBranch {
			print('branch ')
			b.fn_print_oid(fid, inst.cond)
			then_bb := b.basic_blocks[inst.then_bb]
			else_bb := b.basic_blocks[inst.else_bb]
			print(' ? ${then_bb.label}_${then_bb.id}')
			if inst.then_args.len > 0 {
				print('(')
				for i, arg in inst.then_args {
					if i > 0 {
						print(', ')
					}
					b.fn_print_oid(fid, arg)
				}
				print(')')
			}
			print(' : ${else_bb.label}_${else_bb.id}')
			if inst.else_args.len > 0 {
				print('(')
				for i, arg in inst.else_args {
					if i > 0 {
						print(', ')
					}
					b.fn_print_oid(fid, arg)
				}
				print(')')
			}
		}
		IRReturn {
			print('return')
			if value := inst.value {
				print(' ')
				b.fn_print_oid(fid, value)
			}
		}
		IRMacroLiteralCmd {
			print('macro_cmd $')
			for part in inst.parts {
				match part {
					IRMacroCmdText {
						print(part.text)
					}
					IRMacroCmdMacro {
						if part.is_ref {
							print('$(')
						} else {
							print('#(')
						}
						b.fn_print_rid(fid, part.value)
						print(')')
					}
					IRMacroCmdString {
						print('"')
						for str_part in part.parts {
							match str_part {
								IRStringText {
									print(str_part.text)
								}
								IRStringMacro {
									print('#(rid)')
								}
							}
						}
						print('"')
					}
				}
			}
		}
	}
}
