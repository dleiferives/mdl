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
	insts           []IRInstruction
	refs            []IRRef
	refs_map        map[RID]VID
	vars_map        map[string]VID
	consts          []IRConstant
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

// TODO: we need to add scopes to basic blocks, so we can
// who own whom. and who we can access.
// Also need to determine
pub struct IRBasicBlock {
pub mut:
	label     string
	id        BBID
	namespace NID
	function  FID
	args      []IRBasicBlockArg
	insts     []IID
	vars_map  map[string]RID // Variable name -> RID mapping for this block

	// Relationships
	dominator       ?BBID
	dominance_level int
	predecessors    []BBID
	successors      []BBID

	// For construction
	stmts     []Stmt // The list of statements from the IR that form this basic block
	is_sealed bool   // Are all the predecessors known?
	is_filled bool   // Have we generated alled the instructions?
}

pub type BBAID = int

pub struct IRBasicBlockArg {
pub mut:
	name    string
	id      BBID
	storage StorageKind
	typ     IRType
}

type IID = int
pub type IRInstruction = IRMacroLiteralCmd
	| IRDefine
	| IRTypedDefine
	| IRBinaryOp
	| IRUnaryOp
	| IRAssign
	| IRStore
	| IRCall
	| IRStructInit
	| IRFieldAccess
	| IRIndexAccess
	| IRDeref
	| IRRefInst
	| IRJump
	| IRBranch
	| IRReturn

pub struct IRDefine {
pub mut:
	id     IID
	result RID
	value  OID
	pos    int // Source position for error reporting
}

pub struct IRTypedDefine {
pub mut:
	id     IID
	result RID
	pos    int // Source position for error reporting
}

pub enum AssignOp {
	assign
	add_assign
	sub_assign
	mul_assign
	div_assign
	mod_assign
	swap
}

pub fn (op AssignOp) print() {
	match op {
		.assign { print(' = ') }
		.add_assign { print(' += ') }
		.sub_assign { print(' -= ') }
		.mul_assign { print(' *= ') }
		.div_assign { print(' /= ') }
		.mod_assign { print(' %= ') }
		.swap { print(' >< ') }
	}
}

pub struct IRAssign {
pub mut:
	id     IID
	result RID
	op     AssignOp
	value  OID
	pos    int // Source position for error reporting
}

pub struct IRStore {
pub mut:
	id     IID
	result RID
	source RID
	pos    int // Source position for error reporting
}

pub enum BinaryOp {
	add
	sub
	mul
	div
	mod
	eq
	ne
	lt
	le
	gt
	ge
}

pub struct IRBinaryOp {
pub mut:
	result RID
	id     IID
	op     BinaryOp
	left   OID
	right  OID
	pos    int // Source position for error reporting
}

pub enum UnaryOp {
	neg
	ref
	deref
}

pub struct IRUnaryOp {
pub mut:
	id      IID
	result  RID
	op      UnaryOp
	operand OID
	pos     int // Source position for error reporting
}

pub struct IRCall {
pub mut:
	id       IID
	result   ?RID
	function FID
	args     []OID
	pos      int // Source position for error reporting
}

pub struct IRStructInit {
pub mut:
	id           IID
	result       RID
	struct_type  SID
	field_values map[string]OID
	pos          int // Source position for error reporting
}

pub struct IRFieldAccess {
pub mut:
	id     IID
	result RID
	source RID
	field  string
	pos    int // Source position for error reporting
}

pub struct IRIndexAccess {
pub mut:
	id       IID
	result   RID
	source   RID
	index    OID
	is_slice bool
	end      ?OID
	pos      int // Source position for error reporting
}

pub struct IRDeref {
pub mut:
	id     IID
	result RID
	source RID
	pos    int // Source position for error reporting
}

// Takes the address/reference of a value
pub struct IRRefInst {
pub mut:
	id     IID
	result RID
	source RID
	pos    int // Source position for error reporting
}

pub struct IRJump {
pub mut:
	id     IID
	target BBID
	args   []OID
	pos    int // Source position for error reporting
}

pub struct IRBranch {
pub mut:
	id        IID
	cond      OID
	then_bb   BBID
	then_args []OID
	else_bb   BBID
	else_args []OID
	pos       int // Source position for error reporting
}

pub struct IRReturn {
pub mut:
	id    IID
	value ?OID
	pos   int // Source position for error reporting
}

// dunno
pub struct IRUnreachable {
}

// $ comand
pub struct IRMacroLiteralCmd {
pub mut:
	func  FID
	id    IID
	parts []IRMacroCmdPart
	pos   int // Source position for error reporting
}

pub type IRMacroCmdPart = IRMacroCmdText | IRMacroCmdMacro | IRMacroCmdString

pub struct IRMacroCmdText {
pub mut:
	text string
}

pub struct IRMacroCmdMacro {
pub mut:
	value  RID
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

// TODO: make it so that all string macros can be any expression just inside of a macro
pub struct IRStringMacro {
pub mut:
	value  RID
	is_ref bool
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
	pos          int        // Source position for error reporting
}

type RID = int
type IRRefSum = VID | IRBasicBlockArg | IRFunctionArg | IID

pub struct IRRef {
pub mut:
	id    RID
	value IRRefSum
	typ   IRType
	pos   int // Source position for error reporting
}

// IR OPERAND
type OID = RID | CID

@[inline]
pub fn (id OID) to_ir_type(f IRFunction) IRType {
	match id {
		RID {
			return f.refs[id].typ
		}
		CID {
			match f.consts[id] {
				IRIntConst {
					return BuiltinType.int_t
				}
				IRCharConst {
					return BuiltinType.char_t
				}
				IRFloatConst {
					return BuiltinType.float_t
				}
				IRStringConst {
					return BuiltinType.string_t
				}
				IRListConst {
					return BuiltinType.list_t
				}
				IRDictConst {
					return BuiltinType.dict_t
				}
				IRRangeConst {
					return BuiltinType.list_t // Ranges are list-like
				}
			}
		}
	}
}

type CID = int
pub type IRConstant = IRIntConst
	| IRFloatConst
	| IRCharConst
	| IRStringConst
	| IRListConst
	| IRDictConst
	| IRRangeConst

pub struct IRIntConst {
pub mut:
	value int
}

pub struct IRCharConst {
pub mut:
	value string
}

pub struct IRFloatConst {
pub mut:
	value f64
}

pub struct IRStringConst {
pub mut:
	parts []IRStringPart // For interpolated strings
}

// Will have to turn all of the OID's into macros for list dict and range consts
pub struct IRListConst {
pub mut:
	elements []OID
}

pub struct IRDictConst {
pub mut:
	entries []IRDictEntry
}

pub struct IRDictEntry {
pub mut:
	key   OID
	value OID
}

pub struct IRRangeConst {
pub mut:
	start ?OID
	end   ?OID
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
	errors       &ErrorManager = unsafe { nil } // Error manager for reporting
}

pub fn (mut b IRBuilder) solve_namespaces() bool {
	mut ns_solver := NSSolver{}
	for file in b.files {
		ns_solver.solve(file) or {
			if b.errors != unsafe { nil } {
				b.errors.error(.namespace_not_found, SourceLocation{
					file: file
					pos:  0
				}, 'could not resolve namespace from file: ${file}')
			}
			return false
		}
	}
	if !ns_solver.verify_legal() {
		if b.errors != unsafe { nil } {
			b.errors.error(.namespace_collision, empty_location(), 'could not solve namespaces: circular dependency or naming conflict detected')
		}
		return false
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
		if b.errors != unsafe { nil } {
			b.errors.error(.namespace_not_found, empty_location(), 'no namespaces found in source files')
		}
		return false
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
	return true
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
	match typ {
		BuiltinType {
			return IRType(typ)
		}
		ReferenceType {
			// Recursively resolve the base type and wrap in IRRefType
			base_ir := b.namespace_yeild_ir_type(ns, typ.base) or { return none }
			return IRType(IRRefType{
				base: base_ir
			})
		}
		StructType {
			ns_path := typ.name.to_list()
			nnid := b.tranverse_namespace(ns, ns_path) or {
				println('Could not traverse from ${ns.name} along ${ns_path} for type conversion ${typ}')
				return none
			}
			// Need to check if name is a struct in that namespace
			sid := b.namespaces[nnid].struct_map[typ.name.name.name] or {
				println('struct is not found in that namespace to use as a field')
				return none
			}
			return IRType(sid)
		}
	}
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
			mut s := &b.structs[smap[astpos]] or {
				if b.errors != unsafe { nil } {
					b.errors.error(.internal_compiler_error, empty_location(), 'internal error: struct not found in mapping during field resolution')
				}
				result = false
				continue
			}

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
						if b.errors != unsafe { nil } {
							b.errors.error(.cannot_resolve_type, SourceLocation{
								file: ''
								pos:  ast_s.Node.pos
							}, 'unsupported type in struct field: ${ft}')
						}
						result = false
						continue
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
								if b.errors != unsafe { nil } {
									b.errors.error(.invalid_operation, SourceLocation{
										pos: stmt.Node.pos
									}, 'ephemeral storage is not allowed at namespace scope')
								}
								result = false
								continue
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
	if !b.solve_namespaces() {
		return false
	}

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

pub fn (mut b IRBuilder) link_bb(self BBID, child BBID) {
	// TODO: add some saftey checks lol
	b.basic_blocks[self].successors << child
	b.basic_blocks[child].predecessors << self
}

pub fn (mut b IRBuilder) create_bb(fid FID, label string) BBID {
	nid := b.functions[fid].namespace
	bb := IRBasicBlock{
		label:     label
		id:        b.basic_blocks.len
		namespace: nid
		function:  fid
		is_sealed: false
		is_filled: false
	}
	bbid := bb.id
	b.basic_blocks << bb
	b.functions[fid].bbs << bbid
	return bbid
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
		FloatLiteral {
			return false
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

pub fn (mut b IRBuilder) literal_needs_block(l Literal) bool {
	match l {
		IntegerLiteral {
			return false
		}
		FloatLiteral {
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
			return true
		}
		DictionaryLiteral {
			return true
		}
		RangeLiteral {
			return true
		}
	}
	return false
}

pub enum Expr_case {
	needs_block
	has_macro
}

pub fn (mut b IRBuilder) expr_case_looker(e Expr, kind Expr_case) bool {
	match e {
		BinaryExpr {
			return b.expr_case_looker(e.left, kind) || b.expr_case_looker(e.right, kind)
		}
		UnaryExpr {
			return b.expr_case_looker(e.right, kind)
		}
		Literal {
			match kind {
				.needs_block {
					return b.literal_needs_block(e)
				}
				.has_macro {
					return b.literal_has_macro(e)
				}
			}
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
					e1 := b.expr_case_looker(a.target, kind)
					e2 := b.expr_case_looker(a.index.index_expr, kind)
					if a.index.is_slice {
						ee := a.index.slice_end or { return e1 || e2 }

						return b.expr_case_looker(ee, kind) || e1 || e2
					}
					return e1 || e2
				}
				MemberAccessExpr {
					e1 := b.expr_case_looker(a.target, kind)
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
								e2 := b.expr_case_looker(cae.index_expr, kind)
								if e2 {
									return true
								}
								if cae.is_slice {
									e3 := cae.slice_end or { continue }

									if b.expr_case_looker(e3, kind) {
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
					if b.expr_case_looker(a.base_target, kind) {
						return true
					}
					for arg in a.args {
						if b.expr_case_looker(arg, kind) {
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

pub fn (mut b IRBuilder) expr_has_macro(e Expr) bool {
	return b.expr_case_looker(e, .has_macro)
}

pub fn (mut b IRBuilder) expr_needs_block(e Expr) bool {
	return b.expr_case_looker(e, .needs_block)
}

// Start is the BBID to start parsing on
// stmts are the statements that belong to this block or its children
// Returns the BBID of the block that it has ended on
// TODO: rip out after I finish
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
				if b.errors != unsafe { nil } {
					b.errors.error(.not_implemented, SourceLocation{
						pos: stmt.Node.pos
					}, 'inline function definitions are not yet implemented')
				}
				result = false
			}
			Block {
				if b.errors != unsafe { nil } {
					b.errors.error(.unsupported_statement_location, SourceLocation{
						pos: stmt.Node.pos
					}, 'bare block statements are not allowed inside function bodies')
				}
				result = false
			}
			IfStmt {
				// Both the condition and of course the blocks... will create blocks
				if b.expr_needs_block(stmt.condition) {
					cond := b.create_bb(fid, 'if_cond')
					b.link_bb(current, cond)
					current = cond
				}
				b.basic_blocks[current].stmts << stmt
				// create then else and merge block
				mut then := b.create_bb(fid, 'if_then')
				b.link_bb(current, then)
				state, then = b.bb_build_bb_cfg(then, stmt.then_block.stmts)
				result = result && state
				mut el := b.create_bb(fid, 'if_else')
				b.link_bb(current, el)
				if stmt.else_block == none {
				} else {
					elb := stmt.else_block
					state, el = b.bb_build_bb_cfg(el, elb.stmts)
					result = result && state
				}
				merge := b.create_bb(fid, 'if_merge')
				b.link_bb(then, merge)
				b.link_bb(el, merge)
				current = merge
			}
			Return {
				ret := stmt.value or {
					b.basic_blocks[current].stmts << stmt
					continue
				}

				if b.expr_needs_block(ret) {
					ret_bbid := b.create_bb(fid, 'ret')
					b.link_bb(current, ret_bbid)
					current = ret_bbid
				}
				b.basic_blocks[current].stmts << stmt
			}
			ExprStmt {
				if b.expr_needs_block(stmt.expr) {
					emac := b.create_bb(fid, 'macro')
					b.link_bb(current, emac)
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
					command := b.create_bb(fid, 'command')
					b.link_bb(current, command)
					current = command
				}
				b.basic_blocks[current].stmts << stmt
			}
			Define {
				if b.expr_needs_block(stmt.value) {
					macro_use := b.create_bb(fid, 'macro_use')
					b.link_bb(current, macro_use)
					current = macro_use
				}
				b.basic_blocks[current].stmts << stmt
			}
			Assignment {
				if b.expr_needs_block(stmt.left) || b.expr_needs_block(stmt.right) {
					assignment := b.create_bb(fid, 'assignment')
					b.link_bb(current, assignment)
					current = assignment
				}
				b.basic_blocks[current].stmts << stmt
			}
			Store {
				if b.expr_needs_block(stmt.left) || b.expr_needs_block(stmt.right) {
					store := b.create_bb(fid, 'store')
					b.link_bb(current, store)
					current = store
				}
				b.basic_blocks[current].stmts << stmt
			}
		}
	}
	return result, current
}

// Build CFG recursively, creating new basic blocks when macro values are used.
// Each basic block maps to a .mcfunction file - when macros are used, we need
// a new function file to pass the macro arguments.
pub fn (mut b IRBuilder) build_cfg_recursive(fid FID, current_bb BBID, stmts []Stmt) ?(bool, BBID) {
	mut bb := current_bb

	for stmt in stmts {
		match stmt {
			// TypedDefine: No value expression, no macro possible - just attach
			TypedDefine {
				b.basic_blocks[bb].stmts << stmt
			}
			// Define: Check if RHS has/is a macro
			Define {
				if b.expr_has_macro(stmt.value) {
					def_bb := b.create_bb(fid, 'define_macro')
					b.link_bb(bb, def_bb)
					b.basic_blocks[bb].is_sealed = true
					bb = def_bb
				}
				b.basic_blocks[bb].stmts << stmt
			}
			// Assignment: Check both sides for macros
			Assignment {
				if b.expr_has_macro(stmt.left) || b.expr_has_macro(stmt.right) {
					assign_bb := b.create_bb(fid, 'assign_macro')
					b.link_bb(bb, assign_bb)
					b.basic_blocks[bb].is_sealed = true
					bb = assign_bb
				}
				b.basic_blocks[bb].stmts << stmt
			}
			// Store: Same as assignment
			Store {
				if b.expr_has_macro(stmt.left) || b.expr_has_macro(stmt.right) {
					store_bb := b.create_bb(fid, 'store_macro')
					b.link_bb(bb, store_bb)
					b.basic_blocks[bb].is_sealed = true
					bb = store_bb
				}
				b.basic_blocks[bb].stmts << stmt
			}
			// ExprStmt: Expression that may contain macros
			ExprStmt {
				if b.expr_has_macro(stmt.expr) {
					expr_bb := b.create_bb(fid, 'expr_macro')
					b.link_bb(bb, expr_bb)
					b.basic_blocks[bb].is_sealed = true
					bb = expr_bb
				}
				b.basic_blocks[bb].stmts << stmt
			}
			// IfStmt: Creates branches in CFG
			IfStmt {
				// Check if condition has macro
				if b.expr_has_macro(stmt.condition) {
					cond_bb := b.create_bb(fid, 'if_cond')
					b.link_bb(bb, cond_bb)
					b.basic_blocks[bb].is_sealed = true
					bb = cond_bb
				}

				// Store the if statement (condition evaluation) in current block
				b.basic_blocks[bb].stmts << stmt
				b.basic_blocks[bb].is_sealed = true

				// Create then branch
				then_bb := b.create_bb(fid, 'if_then')
				b.link_bb(bb, then_bb)
				_, then_end := b.build_cfg_recursive(fid, then_bb, stmt.then_block.stmts)?

				// Create else branch
				else_bb := b.create_bb(fid, 'if_else')
				b.link_bb(bb, else_bb)

				mut else_end := else_bb
				if else_block := stmt.else_block {
					_, else_end = b.build_cfg_recursive(fid, else_bb, else_block.stmts)?
				} else {
					b.basic_blocks[else_bb].is_sealed = true
				}

				// Create merge point
				merge_bb := b.create_bb(fid, 'if_merge')
				b.link_bb(then_end, merge_bb)
				b.link_bb(else_end, merge_bb)

				bb = merge_bb
			}
			// Return: Terminal instruction
			Return {
				// Check if return value has macro
				if val := stmt.value {
					if b.expr_has_macro(val) {
						ret_bb := b.create_bb(fid, 'return_eval')
						b.link_bb(bb, ret_bb)
						b.basic_blocks[bb].is_sealed = true
						bb = ret_bb
					}
				}
				b.basic_blocks[bb].stmts << stmt
				b.basic_blocks[bb].is_sealed = true
				return true, bb
			}
			// MacroLiteralCommand: $ commands with potential macros
			MacroLiteralCommand {
				mut has_macro := false

				for part in stmt.parts {
					if part is MacroLiteralMacro {
						has_macro = true
						break
					}
					if part is MacroLiteralString {
						// String could have interpolation with macros
						if b.literal_has_macro(Literal(part.str_literal)) {
							has_macro = true
							break
						}
					}
				}

				if has_macro {
					cmd_bb := b.create_bb(fid, 'command')
					b.link_bb(bb, cmd_bb)
					b.basic_blocks[bb].is_sealed = true
					bb = cmd_bb
				}
				b.basic_blocks[bb].stmts << stmt
			}
			// Invalid statements in function bodies
			StructDefinition, FunctionDefinition, NamespaceDefinition, NamespaceImport,
			NamespaceAlias, FunctionInlineDefinition, Block {
				println('Error: ${stmt} not allowed in function body')
				return none
			}
		}
	}

	b.basic_blocks[bb].is_sealed = true
	return true, bb
}

pub fn (mut b IRBuilder) s2_phase1_build_cfg_skeleton(fid FID) bool {
	func := b.functions[fid]

	// Create our entry block
	entry_bb := b.create_bb(fid, 'entry')
	b.functions[fid].entrybb = entry_bb

	_, end_bb := b.build_cfg_recursive(fid, entry_bb, func.block.stmts) or {
		// TODO: change this to be a proper error
		println('We could not build the CFG for ${func.name}')
		return false
	}

	// Adding a return here, so that we will always have a return at the end of our functions, so we just like. know that
	b.basic_blocks[entry_bb].is_sealed = true
	if b.basic_blocks[end_bb].insts.len == 0 {
		b.basic_blocks[end_bb].stmts << Stmt(Return{
			value: none
		})
	}

	return true
}

// TODO: add the error classes so we can accumulate errors and print out helpfully.
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

	// Phase 1: Build CFG skeletons for all functions
	for fid in 0 .. b.functions.len {
		if !b.s2_phase1_build_cfg_skeleton(fid) {
			return false
		}
	}

	// Phase 2: Solve basic block arguments (for macro value passing)
	for fid in 0 .. b.functions.len {
		b.solve_bb_arguments(fid)
	}

	// Phase 3: Lower instructions and add terminators
	for fid in 0 .. b.functions.len {
		b.fill_function_insts(fid)
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

	// Run semantic analysis after IR is built
	mut checker := SemanticChecker.new(&b)
	if !checker.check() {
		// Errors are collected in the shared ErrorManager, main will print them
		return false
	}

	return true
}

pub fn (mut b IRBuilder) lower_namespace_alias(nsa NamespaceAlias) {
}

pub fn (mut b IRBuilder) lower_stmt(stmt Stmt) bool {
	match stmt {
		NamespaceAlias {
			b.lower_namespace_alias(stmt)
			return true
		}
		else {
			if b.errors != unsafe { nil } {
				b.errors.error(.not_implemented, empty_location(), 'statement lowering not implemented for: ${stmt}')
			}
			return false
		}
	}
}
