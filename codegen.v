// module main

// fn main() {
// 	example()
// 	print('\n')
// }

// fn example() {
// 	str := 'reg a := 10;
// reg b := 10;
// reg c := a + b;
// '
// 	res := parse(str)
// 	print(res)
// 	mut fun := Function{
// 		name: 'test'
// 	}
// 	fun.init()

// 	generate_expr(res[0], res, mut &fun)
// 	generate_expr(res[1], res, mut &fun)
// 	generate_expr(res[2], res, mut &fun)
// 	print(fun)
// 	print(generate_function(mut &fun))
// }

// fn generate_function(mut fun Function) string {
// 	mut str := ''
// 	for inst in fun.insts.list {
// 		str += generate_inst(inst, mut fun) + '\n'
// 	}
// 	return str
// }

// fn generate_inst(inst Instruction, mut fun Function) string {
// 	k := inst.kind
// 	mut str := ''
// 	match k {
// 		Set {
// 			str = generate_set(s, mut fun)
// 		}
// 		else {
// 			print('no type ${k}\n')
// 		}
// 	}
// 	return str
// }

// enum GenRefKind {
// 	set
// 	get
// }

// fn generate_set(s Set, mut fun Function) string {
// 	mut str := ''
// 	assert s.dst.kind != .immediate
// 	assert s.dst.kind != .illegal
// 	assert s.dst.source != .effemeral
// 	assert s.src.kind != .illegal
// 	match s.dst.source {
// 		.real {
// 			panic('cannot be real in generating dest, must collapse first')
// 		}
// 		.register {
// 			match s.src.kind {
// 				.immediate {
// 					match s.src.type {
// 						.int {
// 							str = 'scoreboard players set ' + generate_ref(s.dst, mut fun) + ' ' +
// 								fun.name + ' ' + s.src.imm.str()
// 						}
// 						else {
// 							// TODO: refactor / remove this
// 							str = 'data modify storage ' + generate_ref(s.src, mut fun) +
// 								' set value'
// 							s.src.imm.str() + '\n'
// 						}
// 					}
// 					return str
// 				}
// 			}
// 			match s.src.source {
// 				.effemeral { panic('illegal') }
// 				.register { return 'execute' }
// 				.data {}
// 				.real { panic('illegal real in src') }
// 			}
// 		}
// 		.data {}
// 		.effemeral {}
// 	}
// 	return str
// }

// fn generate_set(s Set, mut fun Function) string {
// 	mut str := ''
// 	assert s.dst.kind != .immediate
// 	assert s.dst.kind != .illegal
// 	assert s.dst.source != .effemeral
// 	match s.dst.source {
// 		.real, .register {}
// 		.data {}
// 		else {}
// 	}
// 	match s.src.source {
// 		.effemeral {}
// 		else {
// 			match s.src.kind {
// 				.immediate {}
// 				else {}
// 			}
// 		}
// 	}

// 	return str
// }

// fn generate_ref(r Ref, mut fun Function) string {
// 	mut str := ''
// 	match r.source {
// 		.register {
// 			mut name := if r.name != '' { r.name } else { '_' }
// 			name += r.id.str()
// 			return name
// 		}
// 		.data {
// 			mut name := if r.name != '' { r.name } else { '_' }
// 			name += r.id.str()
// 			str = 'example:utils ' + fun.name + '.' + name
// 		}
// 		else {
// 			print('')
// 		}
// 	}
// 	return str
// }
