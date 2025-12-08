module main

fn main() {
	mut irb := IRBuilder{}
	irb.lower(['tests/ir/bb_gen/stmts/typed_define.mcf'])!
	irb.print_cfg_dot()
	for bb in irb.basic_blocks {
		irb.fill_bb_insts(bb.id)
		print(irb.functions[bb.function].insts)
	}

	// for func in irb.functions {
	// 	println('func ${func.name}\n ${func.block}')
	// }
}
