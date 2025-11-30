module main

pub fn test_compile() {
	mut a := IRBuilder{}
	a.ingest_file('tests/ir/namespace/simple/main.mcf')
	assert true == false
}
