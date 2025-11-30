module main

import os

@[heap]
pub struct IRNamespace {
pub mut:
	name     string // The name of this namespace
	children map[string]&IRNamespace
	parent   ?&IRNamespace
}

pub struct IRBuilder {
pub mut:
	srcs map[string][]Stmt
}

pub fn (mut b IRBuilder) ingest_file(path string) {
	p := os.abs_path(path)
	mut ast := parse_file(p)
	println(os.split_path(p))
	b.srcs[path] = ast
	print(b.srcs)
}

pub fn (mut b IRBuilder) lower() {
	// pass 1 : solve namespaces
	// 1.1: Find all required things
	// 1.2: Then solve

	// pass 2 : solve functions and namespace locals
	// pass 3 : Lower the code
}
