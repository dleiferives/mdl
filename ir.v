module main

// NamespaceAlias not allowed as child of block. at any scale.
import os
import datatypes
import arrays

// ID (index) into the namespaces array in IRBuilder
type NID = int

@[heap]
pub struct IRNamespace {
pub mut:
	name          string // The name of this namespace
	id            NID
	parent        ?NID
	children_list []NID
	linked_list   []NID
	nsmap         map[string]NID
	nsumap        map[NID]string // used for local form of import or whatever
	functions     []FID
	structs       []SID
	variables     []VID
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

pub type IRType = IRBuiltinType | IRRefType | SID

pub enum IRBuiltinType {
	int_t
	float_t
	list_t
	string_t
	dict_t
	void_t
}

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
	srcs       map[string][]Stmt
	base_file  string
	namespaces []IRNamespace
	functions  []IRNamespace
	structs    []IRStructDef
	variables  []IRValue
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
