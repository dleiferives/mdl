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
	match ref.value {
		VID {}
		IRBasicBlockArg {}
		IRFunctionArg {}
		IID {}
	}
}
