module main

fn main() {
	mut irb := IRBuilder{}
	irb.lower(['tests/ir/bb_gen/stmts/assign.mcf'])!
	irb.print_cfg_dot()
	for mut bb in irb.basic_blocks {
		irb.fill_bb_insts(bb.id)
		print(irb.functions[bb.function].insts)
		print('\n')
		print(bb.insts)
		print('\n')
		for inst in bb.insts {
			irb.fn_print_inst(bb.function, inst)
			print('\n')
		}
		// print(irb.functions[bb.function].block)
	}

	// for func in irb.functions {
	// 	println('func ${func.name}\n ${func.block}')
	// }
}
