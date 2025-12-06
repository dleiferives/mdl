module main

import os

// NamespaceAlias not allowed as child of block. at any scale.

// ID (index) into the namespaces array in IRBuilder
type NID = int

@[heap]
pub struct IRNamespace {
pub mut:
	name         string // The name of this namespace
	id           NID
	parent       ?NID
	children     []NID
	linked       []NID
	nsmap        map[string]NID
	nsumap       map[NID]string // used for local form of import or whatever
	functions    []FID
	function_map map[string]FID
	structs      []SID
	struct_map   map[string]SID
	variables    []VID
	variable_map map[string]VID
	block        Block
}

// ID (index) into the Functions array in IRBuilder
type FID = int

@[heap]
pub struct IRFunction {
pub mut:
	name            string
	id              FID
	namespace       NID
	args            []IRFunctionArg
	return_type     IRType
	bbs             []BBID
	entrybb         BBID
	is_inline       bool  // Later
	inline_defaults []int // Later
	block           Block
}

pub struct IRFunctionArg {
pub mut:
	name    string
	storage StorageKind
	typ     IRType
}

type BBID = int

// TODO: resolve this lol
pub struct IRBasicBlock {
pub mut:
	label        string
	id           BBID
	namespace    NID
	function     FID
	args         []IRBasicBlockArg
	insts        []IID
	predecessors []BBID
	successors   []BBID
	stmts        []Stmt // The list of statements from the IR that form this basic block
}

pub struct IRBasicBlockArg {
}

type IID = int

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
	is_macro_dep bool       // If it is the result of a macro call
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
	files        []string
	namespaces   []IRNamespace
	functions    []IRFunction
	basic_blocks []IRBasicBlock
	structs      []IRStructDef
	variables    []IRValue
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

pub fn (mut b IRBuilder) namespace_yeild_ir_type(ns IRNamespace, typ Type) ?IRType {
	mut ft := typ
	mut it := IRType{}
	mut ctr := 0
	for mut ft is ReferenceType {
		ft = ft.base
		ctr++
	}
	match typ {
		BuiltinType {
			it = IRType(typ as BuiltinType)
			for _ in 0 .. ctr {
				it = IRType(IRRefType{
					base: it
				})
			}
			return it
		}
		StructType {
			sft := (typ as StructType)
			ns_path := sft.name.to_list()
			nnid := b.tranverse_namespace(ns, ns_path) or {
				println('Could to traverse from ${ns.name} along ${ns_path} for type conversion ${typ} ')
				return none
			}
			// Need to check if name is a struct in that namespace
			it = b.namespaces[nnid].struct_map[sft.name.name.name] or {
				println('struct is not foundd in that namespace to use as a field')
				return none
			}
			for _ in 0 .. ctr {
				it = IRType(IRRefType{
					base: it
				})
			}
			return it
		}
		else {
			panic('unreachable')
		}
	}
	return none
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

pub fn (mut b IRBuilder) namespace_extract_func_headers() bool {
	mut result := true
	for ns in b.namespaces {
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
				name:        ast_f.ident.name
				id:          b.functions.len
				return_type: b.namespace_yeild_ir_type(ns, ast_f.return_type) or {
					println('Could not resolve function definition ${ast_f}')
					result = false
					continue
				}
				block:       ast_f.block
			}

			// Do the arguments
			for arg in ast_f.args {
				func.args << IRFunctionArg{
					name:    arg.name.name
					storage: arg.source.to_ir()
					typ:     b.namespace_yeild_ir_type(ns, arg.arg_type) or {
						println('Could not resolve argument type ${arg} in function ${ast_f}')
						result = false
						continue
					}
				}
			}
			b.namespaces[ns.id].functions << func.id
			b.namespaces[ns.id].function_map[func.name] = func.id
			b.functions << func
		}
	}
	return result
}

pub fn (mut b IRBuilder) namespace_extract_namesapce_defs() bool {
	mut result := true
	for ns in b.namespaces {
		for stmt in ns.block.stmts {
			match stmt {
				Define {
					println('Warning ${stmt} defining with value is not allowed at namespace scope. only allowed to use type define')
					result = false
					continue
				}
				TypedDefine {
					var := IRValue{
						name:     stmt.name.name
						id:       b.variables.len
						typ:      b.namespace_yeild_ir_type(ns, stmt.typ) or {
							println('Could not resolve type for namespace local ${stmt}')
							result = false
							continue
						}
						storage:  stmt.source.to_ir()
						location: match stmt.source {
							.register {
								IRRegLocation{
									namespace: ns.id
								}
							}
							.data {
								IRDataLocation{
									namespace: ns.id
								}
							}
							else {
								panic('unreachable effemeral as namespace value')
							}
						}
					}
					b.namespaces[ns.id].variables << var.id
					b.namespaces[ns.id].variable_map[var.name] = var.id
					b.variables << var
				}
				else {
					continue
				}
			}
		}
	}
	return result
}

pub fn (mut b IRBuilder) namespace_check_names_unique() bool {
	mut result := true
	for ns in b.namespaces {
		mut names := []string{}
		for f in ns.function_map.keys() {
			if f in names {
				println('Name ${f} already defined in ns ${ns}')
				result = false
				continue
			}
			names << f
		}
		for f in ns.nsmap.keys() {
			if f in names {
				println('Name ${f} already defined in ns ${ns}')
				result = false
				continue
			}
			names << f
		}
		for f in ns.struct_map.keys() {
			if f in names {
				println('Name ${f} already defined in ns ${ns}')
				result = false
				continue
			}
			names << f
		}
		for f in ns.variable_map.keys() {
			if f in names {
				println('Name ${f} already defined in ns ${ns}')
				result = false
				continue
			}
			names << f
		}
	}
	return result
}

// The first stage of the IR passes. This really is just basically doing early
// stages of type checking, as well as setting up the IRBuilder structure to
// allow for the easier computation of the basic blocks and other more advanced
// control flow. Namely PHI nodes when and if I get to that.
@[inline]
pub fn (mut b IRBuilder) stage1() bool {
	mut result := true
	// We are going to do this in several passes. The primary reason for this is
	// to just limit the scope of the problem so that it becomes easier to work
	// with. To start we are going to break up all the namespaces and make good
	// references for all them to each other.
	b.solve_namespaces()

	// Next we are going to pull out all the function definitions, struct
	// declarations, for all the namespaces. We have to start with the structs
	// so that our types can been resolved.
	result = b.namespace_extract_structs() && result

	// Now we are going to handle functions! That is function definitions. Since
	// we've setup the architechture for this it should go down easy.
	result = b.namespace_extract_func_headers() && result

	// Now let us handle the definitions that exist in the namespace scope
	result = b.namespace_extract_namesapce_defs() && result

	// Now lets be sure that we don't have anything that is sharing the same name
	result = b.namespace_check_names_unique() && result
	return result
}

pub fn (mut b IRBuilder) bb_link(self BBID, child BBID) {
	// TODO: add some saftey checks lol
	b.basic_blocks[self].successors << child
	b.basic_blocks[child].predecessors << self
}

pub fn (mut b IRBuilder) add_bb(fid FID, label string) BBID {
	nid := b.functions[fid].namespace
	bb := IRBasicBlock{
		label:     label
		id:        b.basic_blocks.len
		namespace: nid
		function:  fid
	}
	b.basic_blocks << bb
	b.functions[fid].bbs << bb.id
	return bb.id
}

pub fn (mut b IRBuilder) literal_has_macro(l Literal) bool {
	match l {
		IntegerLiteral {
			return false
		}
		StringLiteral {
			if !l.interpolated {
				return false
			}
			for p in l.parts {
				if p.is_macro {
					return true
				}
			}
		}
		CharLiteral {
			return false
		}
		ListLiteral {
			for e in l.elements {
				if b.expr_has_macro(e) {
					return true
				}
			}
		}
		DictionaryLiteral {
			for ent in l.entries {
				match ent.key_kind {
					.integer_key {
						return false
					}
					.macro_key {
						return true
					}
					.string_key {
						return b.literal_has_macro(ent.string_key or {
							println('Could not resolve string key but should be able to ${ent}')
							continue
						})
					}
				}
			}
		}
		RangeLiteral {
			s := l.start or {
				e := l.end or {
					println('Illegal state on range ${l} cannot have both start and end as none')
					return false
				}

				return b.expr_has_macro(e)
			}

			e := l.end or { return b.expr_has_macro(s) }
			return b.expr_has_macro(e) || b.expr_has_macro(s)
		}
	}
	return false
}

pub fn (mut b IRBuilder) expr_has_macro(e Expr) bool {
	match e {
		BinaryExpr {
			return b.expr_has_macro(e.left) || b.expr_has_macro(e.right)
		}
		UnaryExpr {
			return b.expr_has_macro(e.right)
		}
		Literal {
			return b.literal_has_macro(e)
		}
		Identifier {
			return false
		}
		MacroExpr {
			return true
		}
		AccessExpr {
			a := e
			match a {
				IndexAccessExpr {
					e1 := b.expr_has_macro(a.target)
					e2 := b.expr_has_macro(a.index.index_expr)
					if a.index.is_slice {
						ee := a.index.slice_end or { return e1 || e2 }

						return b.expr_has_macro(ee) || e1 || e2
					}
					return e1 || e2
				}
				MemberAccessExpr {
					e1 := b.expr_has_macro(a.target)
					if e1 {
						return true
					}
					for cae in a.chain {
						match cae {
							FieldAccessElement {
								continue
							}
							MacroAccessElement {
								return true
							}
							IndexAccessElement {
								e2 := b.expr_has_macro(cae.index_expr)
								if e2 {
									return true
								}
								if cae.is_slice {
									e3 := cae.slice_end or { continue }

									if b.expr_has_macro(e3) {
										return true
									}
								}
								continue
							}
							DerefAccessElement {
								continue
							}
						}
					}
				}
				FunctionCallExpr {
					if b.expr_has_macro(a.base_target) {
						return true
					}
					for arg in a.args {
						if b.expr_has_macro(arg) {
							return true
						}
					}
				}
			}
		}
		else {
			print('not implemented')
		}
	}
	return false
}

// Start is the BBID to start parsing on
// stmts are the statements that belong to this block or its children
// Returns the BBID of the block that it has ended on
pub fn (mut b IRBuilder) bb_build_bb_cfg(start BBID, stmts []Stmt) (bool, BBID) {
	fid := b.basic_blocks[start].function
	mut current := start
	mut result := true
	mut state := true
	for stmt in stmts {
		match stmt {
			TypedDefine {
				// Typed defines are not allowed to have any macros in them.
				// As such we will skip them, but we should do a checking pass
				// for them having macros somewhere in them.
				// Just maybe not in this part of this pass.
				b.basic_blocks[current].stmts << stmt
			}
			StructDefinition {
				// Cannot have macro values, should not be included in block, as is not executable
				result = false
				println('Function blocks are not allowed to have struct definitions in them ${stmt}')
			}
			FunctionDefinition {
				// Can have macro values inside of, but this should be handled elsewhere
				// should be ignored here.
				result = false
				println('Function blocks are not allowed to have function definitions in them ${stmt}')
			}
			NamespaceDefinition {
				// Can have macro values inside of, but this should not be handled here
				result = false
				println('Function blocks are not allowed to have namespace definitions in them ${stmt}')
			}
			NamespaceImport {
				// No macro
				result = false
				println('Function blocks are not allowed to have namespace imports in them ${stmt}')
			}
			NamespaceAlias {
				// No macro
				result = false
				println('Function blocks are not allowed to have namespace aliases in them ${stmt}')
			}
			FunctionInlineDefinition {
				println('TODO FunctionInlineDefinition')
				panic('Not handled')
			}
			Block {
				panic('Block in block??')
			}
			IfStmt {
				// Both the condition and of course the blocks... will create blocks
				if b.expr_has_macro(stmt.condition) {
					cond := b.add_bb(fid, 'if_cond')
					b.bb_link(current, cond)
					current = cond
				}
				b.basic_blocks[current].stmts << stmt
				// create then else and merge block
				mut then := b.add_bb(fid, 'if_then')
				b.bb_link(current, then)
				state, then = b.bb_build_bb_cfg(then, stmt.then_block.stmts)
				result = result && state
				mut el := b.add_bb(fid, 'if_else')
				b.bb_link(current, el)
				if stmt.else_block == none {
				} else {
					elb := stmt.else_block
					state, el = b.bb_build_bb_cfg(el, elb.stmts)
					result = result && state
				}
				merge := b.add_bb(fid, 'if_merge')
				b.bb_link(then, merge)
				b.bb_link(el, merge)
				current = merge
			}
			Return {
				ret := stmt.value or {
					b.basic_blocks[current].stmts << stmt
					continue
				}

				if b.expr_has_macro(ret) {
					ret_bbid := b.add_bb(fid, 'ret')
					b.bb_link(current, ret_bbid)
					current = ret_bbid
				}
				b.basic_blocks[current].stmts << stmt
			}
			ExprStmt {
				if b.expr_has_macro(stmt.expr) {
					emac := b.add_bb(fid, 'macro')
					b.bb_link(current, emac)
					current = emac
				}
				b.basic_blocks[current].stmts << stmt
			}
			MacroLiteralCommand {
				parts := stmt.parts
				mut need_block := false
				for p in parts {
					if p is MacroLiteralMacro {
						need_block = true
					}
					if p is MacroLiteralString {
						if b.literal_has_macro(Literal(p.str_literal)) {
							need_block = true
						}
					}
				}
				if need_block {
					command := b.add_bb(fid, 'command')
					b.bb_link(current, command)
					current = command
				}
				b.basic_blocks[current].stmts << stmt
			}
			Define {
				if b.expr_has_macro(stmt.value) {
					macro_use := b.add_bb(fid, 'macro_use')
					b.bb_link(current, macro_use)
					current = macro_use
				}
				b.basic_blocks[current].stmts << stmt
			}
			Assignment {
				if b.expr_has_macro(stmt.left) || b.expr_has_macro(stmt.right) {
					assignment := b.add_bb(fid, 'assignment')
					b.bb_link(current, assignment)
					current = assignment
				}
				b.basic_blocks[current].stmts << stmt
			}
			Store {
				if b.expr_has_macro(stmt.left) || b.expr_has_macro(stmt.right) {
					store := b.add_bb(fid, 'store')
					b.bb_link(current, store)
					current = store
				}
				b.basic_blocks[current].stmts << stmt
			}
		}
	}
	return result, current
}

pub fn (mut b IRBuilder) fn_build_bb_cfg(fid FID) bool {
	println('extracting bbs from function ${b.functions[fid].name}')
	block := b.functions[fid].block
	// Now we are going to make our entry block for this function
	entry := b.add_bb(fid, 'entry')
	valid, _ := b.bb_build_bb_cfg(entry, block.stmts)
	return valid
}

//
pub fn (mut b IRBuilder) stage2() bool {
	mut result := true
	// Here we are going to go through our functions and generate our basic blocks except for the terminal instructions within them.
	// I want to make this be based of the MLIR way they do basic blocks, seems pretty cute ngl
	// look at this for ref https://farena.in/compilers/mlir/ssa-mlir-algorithm/

	// So I've determined that the way that I want to go about doing this is to
	// have it generate the control flow graph first, and then generate the
	// instructions. This means that we just have to pull out all the cases that
	// will produce a control flow graph change. This should be on all if / else
	// statements of course. And on all function calls. As there is no way for
	// doing looping besides recursion for the mooment this should work out
	// fine.
	//
	// For function calls what we will do is create basic blocks for the macro
	// values that will feed into the function call, so that macro values are
	// not actually function calls but rather basic blocks. This will create the
	// seperatation between the mdl code and the mcfunction code. Where each
	// basic block will map to a .mcfunction file.
	//
	// So for every instance that there will be a macro value we will be
	// creating a basic block. We will also store the instructions that will be
	// needed to be parsed for each basic block into each basic block so that it
	// will be easier to handle later.

	// Here we will create the basic blocks for a function
	for fid in 0 .. b.functions.len {
		result = result && b.fn_build_bb_cfg(fid)
	}

	return result
}

pub fn (mut b IRBuilder) lower(files []string) !bool {
	mut abs_files := []string{}
	for f in files {
		abs_files << os.real_path(f)
	}
	b.files = abs_files

	// Do some setup and catch stupid errors Group the stage1 error messages
	// together Its annoying when you get error messages about error messages.
	// Because the type checker just does not have enough information. And it is
	// also annoying (like in zig) where it clearly is throwing each error it
	// runs into. I hope this will be a good middle ground. Though this is
	// probably what zig is doing.
	if !b.stage1() {
		return false
	}
	println('We have ${b.functions.len} Functions')
	for f in b.functions {
		println('${f.name}')
	}
	if !b.stage2() {
		return false
	}

	// Next we are going to check that there is no overlap between names within any namespace
	return true
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
