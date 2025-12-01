module main

import os

// NamespaceAlias not allowed as child of block. at any scale.

// ID (index) into the namespaces array in IRBuilder
type NID = int

@[heap]
pub struct IRNamespace {
pub mut:
	name       string // The name of this namespace
	id         NID
	parent     ?NID
	children   []NID
	linked     []NID
	nsmap      map[string]NID
	nsumap     map[NID]string // used for local form of import or whatever
	functions  []FID
	structs    []SID
	struct_map map[string]SID
	variables  []VID
	block      Block
}

// ID (index) into the Functions array in IRBuilder
type FID = int

@[heap]
pub struct IRFunction {
pub mut:
	name            string
	id              FID
	namespace       NID
	args            []int // todo
	return_type     IRType
	bbs             []int // todo
	entrybb         []int // todo
	is_inline       bool  // Later
	inline_defaults []int // Later
	block           Block
}

pub struct IRFunctionArg {
pub mut:
	name    string
	storage StorageKind
	typ     IRType
	is_ref  bool
}

// ID (index) into the Structs array in IRBuilder
type SID = int

@[heap]
pub struct IRStructDef {
pub mut:
	name      string
	id        SID
	namespace NID
	fields    map[string]IRType
}

pub type IRType = BuiltinType | IRRefType | SID

// pub enum IRBuiltinType {
// 	int_t
// 	float_t
// 	list_t
// 	string_t
// 	dict_t
// 	void_t
// }

pub struct IRRefType {
pub mut:
	base IRType
}

pub enum StorageKind {
	data
	register
	effemeral
}

// VID (index) into the Variables array in IRBuilder
type VID = int

pub struct IRValue {
pub mut:
	name         string
	id           VID
	typ          IRType
	storage      StorageKind
	location     IRLocation // where it is stored (This is our general reference thing)
	is_macro_dep bool       // If it is used in macros
}

// IR OPERAND
pub type IROperand = IRValue | IRConstant

pub type IRConstant = IRIntConst
	| IRFloatConst
	| IRStringConst
	| IRListConst
	| IRDictConst
	| IRRangeConst

pub struct IRIntConst {
pub mut:
	value int
}

pub struct IRFloatConst {
pub mut:
	value f64
}

pub struct IRStringConst {
pub mut:
	value string
	parts []IRStringPart // For interpolated strings
}

pub struct IRListConst {
pub mut:
	elements []IROperand
}

pub struct IRDictConst {
pub mut:
	entries []IRDictEntry
}

pub struct IRDictEntry {
pub mut:
	key   IROperand
	value IROperand
}

pub struct IRRangeConst {
pub mut:
	start ?IROperand
	end   ?IROperand
}

// IR LOCATION

pub type IRLocation = IRRegLocation | IRDataLocation | IREffLocation

pub struct IRRegLocation {
pub mut:
	namespace NID
	function  ?FID
}

pub struct IRDataLocation {
pub mut:
	namespace NID
	function  ?FID
}

pub struct IREffLocation {
pub mut:
	value string // The literal macro value TODO:
}

// $ comand
pub struct IRMacroLiteralCmd {
pub mut:
	parts []IRMacroCmdPart
}

pub type IRMacroCmdPart = IRMacroCmdText | IRMacroCmdMacro | IRMacroCmdString

pub struct IRMacroCmdText {
pub mut:
	text string
}

pub struct IRMacroCmdMacro {
pub mut:
	value  IRValue
	is_ref bool
}

pub struct IRMacroCmdString {
pub mut:
	parts []IRStringPart
}

pub type IRStringPart = IRStringText | IRStringMacro

pub struct IRStringText {
pub mut:
	text string
}

pub struct IRStringMacro {
pub mut:
	value  IRValue
	is_ref bool
}

/// IR BUILDer
@[heap]
pub struct IRBuilder {
pub mut:
	files      []string
	namespaces []IRNamespace
	functions  []IRNamespace
	structs    []IRStructDef
	variables  []IRValue
}

pub fn (mut b IRBuilder) solve_namespaces() {
	mut ns_solver := NSSolver{}
	for file in b.files {
		ns_solver.solve(file) or { panic('Could not solve') }
	}
	if !ns_solver.verify_legal() {
		panic('Could not solve the namespaces... look at that')
	}
	mut s2b := map[NSNodeId]NID{}
	mut b2s := map[NID]NSNodeId{}

	// Create the mapping (and fill out the builder)
	for node in ns_solver.nodes {
		if !node.valid {
			continue
		}
		mut namespace := IRNamespace{
			name:  node.name
			id:    b.namespaces.len
			block: node.block
		}
		b.namespaces << namespace
		s2b[node.id] = namespace.id
		b2s[namespace.id] = node.id
	}

	if b.namespaces.len == 0 {
		panic('There is no namespaces?')
	}

	mut visited := map[NID]bool{}
	for nid, _ in b.namespaces {
		visited[nid] = false
	}

	for nid, _ in b.namespaces {
		if visited[nid] {
			continue
		}
		nsid := b2s[nid]
		ns := ns_solver.nodes[nsid]
		for c in ns.children {
			cnid := s2b[c]
			b.namespaces[nid].children << cnid
			b.namespaces[cnid].parent = nid
			cns := ns_solver.nodes[c]
			b.namespaces[nid].nsmap[cns.name] = cnid
			b.namespaces[nid].nsumap[cnid] = cns.name
		}

		for lid, l in ns.linked {
			lnid := s2b[l]
			b.namespaces[nid].linked << lnid
			lnsn := ns.linked_name[lid]
			b.namespaces[nid].nsmap[lnsn] = lnid
			b.namespaces[nid].nsumap[lnid] = lnsn
		}

		visited[nid] = true
	}
}

pub fn (mut b IRBuilder) tranverse_namespace(nsa IRNamespace, path []string) ?NID {
	mut ns := nsa
	mut nid := 0
	for p in path {
		// TODO: make sure the nsmap has itself for itself.
		nid = ns.nsmap[p] or { return none }
		ns = b.namespaces[nid]
	}
	return nid
}

pub fn (mut b IRBuilder) namespace_extract_structs() bool {
	mut smap := map[int]SID{}
	mut result := true
	for ns in b.namespaces {
		mut ast_structs := []StructDefinition{}
		// Pull out our structs
		for stmt in ns.block.stmts {
			match stmt {
				StructDefinition {
					ast_structs << stmt
				}
				else {}
			}
		}

		struct_const: for ast_s in ast_structs {
			mut s := IRStructDef{
				name:      ast_s.name.name
				id:        b.structs.len
				namespace: ns.id
			}
			smap[ast_s.Node.pos] = s.id
			b.namespaces[ns.id].structs << s.id
			b.namespaces[ns.id].struct_map[s.name] = s.id
			b.structs << s
		}
	}

	for _, ns in b.namespaces {
		mut ast_structs := []StructDefinition{}
		// Pull out our structs
		for stmt in ns.block.stmts {
			match stmt {
				StructDefinition {
					ast_structs << stmt
				}
				else {}
			}
		}

		for ast_s in ast_structs {
			astpos := ast_s.Node.pos
			mut s := &b.structs[smap[astpos]] or { panic('FUCK (IR STRUCT THING)') }

			for f in ast_s.fields {
				mut ft := f.field_type
				mut it := IRType{}
				mut ctr := 0
				for mut ft is ReferenceType {
					ft = ft.base
					ctr++
				}

				match ft {
					BuiltinType {
						it = IRType(ft as BuiltinType)
						for _ in 0 .. ctr {
							it = IRType(IRRefType{
								base: it
							})
						}
						s.fields[f.name.name] = it
					}
					StructType {
						sft := (ft as StructType)
						ns_path := sft.name.to_list()
						nnid := b.tranverse_namespace(ns, ns_path) or {
							println('Could to traverse from ${ns.name} along ${ns_path} for struct ${ast_s}')
							result = false
							continue
						}
						// Need to check if name is a struct in that namespace
						it = b.namespaces[nnid].struct_map[sft.name.name.name] or {
							println('struct is not foundd in that namespace to use as a field')
							result = false
							continue
						}
						for _ in 0 .. ctr {
							it = IRType(IRRefType{
								base: it
							})
						}
						s.fields[f.name.name] = it
					}
					else {
						panic('unreachable type ${ft}')
					}
				}
			}
		}
	}
	return result
}

pub fn (mut b IRBuilder) namespace_extract_func_headers(mut ns IRNamespace) {
	mut ast_funcs := []FunctionDefinition{}
	// Pull out our functions
	for stmt in ns.block.stmts {
		match stmt {
			FunctionDefinition {
				ast_funcs << stmt
			}
			else {}
		}
	}

	for ast_f in ast_funcs {
		mut func := IRFunction{
			name:  ast_f.ident.name
			id:    b.functions.len
			block: ast_f.block
		}
	}
	// TODO: have to finish structs first
}

pub fn (mut b IRBuilder) lower(files []string) !bool {
	mut abs_files := []string{}
	for f in files {
		abs_files << os.real_path(f)
	}
	b.files = abs_files
	// We are going to do this in several passes. The primary reason for this is
	// to just limit the scope of the problem so that it becomes easier to work
	// with. To start we are going to break up all the namespaces and make good
	// references for all them to each other.
	b.solve_namespaces()

	// Next we are going to pull out all the function definitions, struct
	// declarations, for all the namespaces. We have to start with the structs
	// so that our types can been resolved.
	return b.namespace_extract_structs()

	// Next we are going to check that there is no overlap between names within any namespace
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
