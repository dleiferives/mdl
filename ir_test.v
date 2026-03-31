module main

pub fn test_nssolver_simple() {
	mut solver := NSSolver{}
	solver.solve('tests/ir/namespace/simple/main.mdl')!
	println(solver)
	assert solver.verify_legal()
}

pub fn test_nssolver_import() {
	mut solver := NSSolver{}
	solver.solve('tests/ir/namespace/import/a.mdl')!
	println(solver)
	assert solver.verify_legal()
}

pub fn test_nssolver_child_import() {
	mut solver := NSSolver{}
	solver.solve('tests/ir/namespace/child_import/main.mdl')!
	println(solver)
	assert solver.verify_legal()
}

pub fn test_nssolver_child_import_fail() {
	mut solver := NSSolver{}
	solver.solve('tests/ir/namespace/child_import_fail/main.mdl')!
	println(solver)
	assert !solver.verify_legal()
}

pub fn test_struct_single() {
	mut irb := IRBuilder{}
	irb.lower(['tests/ir/structs/simple/main.mdl'])!
	p := irb.structs[0]
	assert p.name == 'Point'
	assert p.fields['x'] is BuiltinType
}

pub fn test_struct_self_ref() {
	mut irb := IRBuilder{}
	irb.lower(['tests/ir/structs/self_ref/main.mdl'])!
	print(irb.structs)
	assert irb.structs[0].fields['next'] == (IRType(IRRefType{
		base: IRType(SID(0))
	}))
}

pub fn test_struct_cross_file() {
	mut irb := IRBuilder{}
	irb.lower(['tests/ir/structs/cross_file/main.mdl'])!
	person := irb.structs[0]
	assert person.name == 'Person'
	assert irb.namespaces[person.namespace].name == 'main'
	x := BuiltinType.int_t
	cm := IRType(IRRefType{
		base: IRType(SID(1))
	})
	assert person.fields['age'] == IRType(x)
	assert person.fields['community'] == cm
	print(irb.structs)
}

pub fn test_struct_cross_file_fail() {
	mut irb := IRBuilder{}
	assert false == irb.lower(['tests/ir/structs/cross_file_fail/main.mdl'])!
}
