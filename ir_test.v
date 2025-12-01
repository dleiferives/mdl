module main

pub fn test_nssolver_simple() {
	mut solver := NSSolver{}
	solver.solve('tests/ir/namespace/simple/main.mcf')!
	println(solver)
	assert solver.verify_legal()
}

pub fn test_nssolver_import() {
	mut solver := NSSolver{}
	solver.solve('tests/ir/namespace/import/a.mcf')!
	println(solver)
	assert solver.verify_legal()
}

pub fn test_nssolver_child_import() {
	mut solver := NSSolver{}
	solver.solve('tests/ir/namespace/child_import/main.mcf')!
	println(solver)
	assert solver.verify_legal()
}

pub fn test_nssolver_child_import_fail() {
	mut solver := NSSolver{}
	solver.solve('tests/ir/namespace/child_import_fail/main.mcf')!
	println(solver)
	assert !solver.verify_legal()
}
