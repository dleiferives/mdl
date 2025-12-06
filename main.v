module main

fn main() {
	mut irb := IRBuilder{}
	irb.lower(['tests/ir/bb_gen/simple/main.mcf'])!
	irb.print_cfg_dot()
	// for func in irb.functions {
	// 	println('func ${func.name}\n ${func.block}')
	// }
}
