module main

// NamespaceAlias not allowed as child of block. at any scale.
import os
import datatypes
import arrays

@[heap]
pub struct IRNamespace {
pub mut:
	name     string // The name of this namespace
	children map[string]&IRNamespace
	parent   ?&IRNamespace
}

pub struct IRBuilder {
pub mut:
	srcs      map[string][]Stmt
	base_file string
}

pub fn (mut b IRBuilder) ingest_file(path string) {
	p := os.abs_path(path)
	mut ast := parse_file(p)
	println(os.split_path(p))
	b.srcs[path] = ast
	print(b.srcs)
}

pub fn (mut b IRBuilder) lower() ! {
	// pass 1 : solve namespaces
	// 1.1: Find all required things
	b.base_file = os.abs_path(b.base_file)
	b.ingest_file(b.base_file)
	mut files_to_search := datatypes.Stack[string]{}
	files_to_search.push(b.base_file)
	mut searched := map[string]bool{}
	searched[b.base_file] = false
	for true {
		mut cfile := files_to_search.pop()!
		if searched[cfile] {
			continue
		}

		mut stmts := datatypes.Stack[Stmt]{}
		for stmt in b.srcs[cfile] {
			stmts.push(stmt)
		}
		mut aliased := false
		mut imports := []NamespaceImport{}
		mut definitions := []NamespaceDefinition{}
		mut alias := ''
		for true {
			mut stmt := stmts.pop()
			match stmt {
				NamespaceAlias {}
				NamespaceImport {}
				NamespaceDefinition {}
				else {}
			}
		}
	}

	// 1.2: Then solve

	// pass 2 : solve functions and namespace locals
	// pass 3 : Lower the code
}

pub fn (mut b IRBuilder) lower_namespace_alias(nsa NamespaceAlias) {
}

pub fn (mut b IRBuilder) lower_stmt(stmt Stmt) {
	match stmt {
		NamespaceAlias {
			b.lower_namespace_alias(stmt)
		}
		else {
			panic('Statement is not implemented ${stmt}')
		}
	}
}
