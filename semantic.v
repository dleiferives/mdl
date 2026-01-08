module main

// ==================== Type Info for Scope Tracking ====================

pub struct TypeInfo {
pub:
	typ     IRType
	storage StorageKind
	pos     int // Where it was defined
}

// ==================== Semantic Context ====================

pub struct SemanticContext {
pub mut:
	current_namespace NID
	current_function  ?FID
	scope_stack       []map[string]TypeInfo
	return_type       ?IRType
	file              string
}

// ==================== Semantic Checker ====================

@[heap]
pub struct SemanticChecker {
pub mut:
	builder &IRBuilder
	errors  &ErrorManager = unsafe { nil } // Shared error manager
	context SemanticContext
}

pub fn SemanticChecker.new(builder &IRBuilder) SemanticChecker {
	return SemanticChecker{
		builder: builder
		errors:  builder.errors // Share error manager with IRBuilder
		context: SemanticContext{
			scope_stack: []map[string]TypeInfo{}
		}
	}
}

// ==================== Error Management ====================

pub fn (mut c SemanticChecker) add_error(kind ErrorKind, pos int, msg string) {
	if c.errors != unsafe { nil } {
		c.errors.error(kind, SourceLocation{
			file: c.context.file
			pos:  pos
		}, msg)
	}
}

pub fn (mut c SemanticChecker) add_error_with_hint(kind ErrorKind, pos int, msg string, hint string) {
	if c.errors != unsafe { nil } {
		c.errors.error_with_hint(kind, SourceLocation{
			file: c.context.file
			pos:  pos
		}, msg, hint)
	}
}

pub fn (mut c SemanticChecker) add_warning(kind ErrorKind, pos int, msg string) {
	if c.errors != unsafe { nil } {
		c.errors.warning(kind, SourceLocation{
			file: c.context.file
			pos:  pos
		}, msg)
	}
}

pub fn (c &SemanticChecker) has_errors() bool {
	if c.errors != unsafe { nil } {
		return c.errors.has_errors()
	}
	return false
}

pub fn (c &SemanticChecker) error_count() int {
	if c.errors != unsafe { nil } {
		return c.errors.error_count
	}
	return 0
}

pub fn (c &SemanticChecker) warning_count() int {
	if c.errors != unsafe { nil } {
		return c.errors.warning_count
	}
	return 0
}

// Print all accumulated errors (delegates to ErrorManager)
pub fn (c &SemanticChecker) print_errors() {
	if c.errors != unsafe { nil } {
		c.errors.print_all()
	}
}

// ==================== Scope Management ====================

pub fn (mut c SemanticChecker) scope_enter() {
	c.context.scope_stack << map[string]TypeInfo{}
}

pub fn (mut c SemanticChecker) scope_exit() {
	if c.context.scope_stack.len > 0 {
		c.context.scope_stack.pop()
	}
}

pub fn (mut c SemanticChecker) scope_define(name string, info TypeInfo) bool {
	if c.context.scope_stack.len == 0 {
		c.scope_enter()
	}

	// Check if already defined in current scope
	idx := c.context.scope_stack.len - 1
	if name in c.context.scope_stack[idx] {
		return false // Already defined
	}

	c.context.scope_stack[idx][name] = info
	return true
}

pub fn (c &SemanticChecker) scope_lookup(name string) ?TypeInfo {
	// Search from innermost to outermost scope
	for i := c.context.scope_stack.len - 1; i >= 0; i-- {
		if name in c.context.scope_stack[i] {
			return c.context.scope_stack[i][name]
		}
	}
	return none
}

// ==================== Type Utilities ====================

pub fn (c &SemanticChecker) types_compatible(expected IRType, actual IRType) bool {
	// Exact match
	if expected == actual {
		return true
	}

	// Check if both are reference types with compatible bases
	match expected {
		IRRefType {
			if actual is IRRefType {
				return c.types_compatible(expected.base, actual.base)
			}
		}
		else {}
	}

	return false
}

pub fn (c &SemanticChecker) type_to_string(typ IRType) string {
	match typ {
		BuiltinType {
			return match typ {
				.int_t { 'Int' }
				.float_t { 'Float' }
				.char_t { 'Char' }
				.string_t { 'String' }
				.list_t { 'List' }
				.dict_t { 'Dict' }
				.void_t { 'Void' }
			}
		}
		IRRefType {
			return '&${c.type_to_string(typ.base)}'
		}
		SID {
			if int(typ) < c.builder.structs.len {
				return c.builder.structs[typ].name
			}
			return 'Struct#${typ}'
		}
	}
}

// ==================== Stage 1 Checks ====================

pub fn (mut c SemanticChecker) check_stage1() bool {
	initial_errors := c.error_count()

	// Set file context from builder
	if c.builder.files.len > 0 {
		c.context.file = c.builder.files[0]
	}

	// Check each namespace
	for ns in c.builder.namespaces {
		c.context.current_namespace = ns.id

		// Check struct definitions
		c.check_struct_definitions(ns)

		// Check function signatures
		c.check_function_signatures(ns)

		// Check name collisions within namespace
		c.check_namespace_names(ns)
	}

	return c.error_count() == initial_errors
}

fn (mut c SemanticChecker) check_struct_definitions(ns IRNamespace) {
	for sid in ns.structs {
		s := c.builder.structs[sid]

		// Check each field has a valid type
		// s.fields is map[string]IRType
		for field_name, field_type in s.fields {
			if !c.is_valid_ir_type(ns, field_type) {
				c.add_error(.undefined_struct, 0,
					"field '${field_name}' in struct '${s.name}' has unknown type")
			}
		}
		// Note: duplicate field names are impossible since fields is a map
	}
}

fn (mut c SemanticChecker) check_function_signatures(ns IRNamespace) {
	for fid in ns.functions {
		func := c.builder.functions[fid]

		// Check return type exists
		if !c.is_valid_ir_type(ns, func.return_type) {
			c.add_error(.undefined_struct, 0,
				"function '${func.name}' has unknown return type")
		}

		// Check argument types exist
		mut seen_args := map[string]bool{}
		for arg in func.args {
			if !c.is_valid_ir_type(ns, arg.typ) {
				c.add_error(.undefined_struct, 0,
					"argument '${arg.name}' in function '${func.name}' has unknown type")
			}

			// Check for duplicate argument names
			if arg.name in seen_args {
				c.add_error(.duplicate_definition, 0,
					"duplicate argument '${arg.name}' in function '${func.name}'")
			}
			seen_args[arg.name] = true
		}
	}
}

fn (mut c SemanticChecker) check_namespace_names(ns IRNamespace) {
	mut all_names := map[string]bool{}

	// Collect function names
	for fid in ns.functions {
		func := c.builder.functions[fid]
		if func.name in all_names {
			c.add_error(.duplicate_definition, 0,
				"'${func.name}' is already defined in namespace '${ns.name}'")
		}
		all_names[func.name] = true
	}

	// Collect struct names
	for sid in ns.structs {
		s := c.builder.structs[sid]
		if s.name in all_names {
			c.add_error(.duplicate_definition, 0,
				"'${s.name}' is already defined in namespace '${ns.name}'")
		}
		all_names[s.name] = true
	}

	// Collect variable names
	for name, _ in ns.variable_map {
		if name in all_names {
			c.add_error(.duplicate_definition, 0,
				"'${name}' is already defined in namespace '${ns.name}'")
		}
		all_names[name] = true
	}
}

fn (c &SemanticChecker) is_valid_type(ns IRNamespace, typ Type) bool {
	match typ {
		BuiltinType {
			return true
		}
		ReferenceType {
			return c.is_valid_type(ns, typ.base)
		}
		StructType {
			// Check if struct exists in namespace or parent namespaces
			// For now, just check if the name is known
			return true // TODO: full resolution
		}
	}
}

fn (c &SemanticChecker) is_valid_ir_type(ns IRNamespace, typ IRType) bool {
	match typ {
		BuiltinType {
			return true
		}
		IRRefType {
			return c.is_valid_ir_type(ns, typ.base)
		}
		SID {
			return int(typ) < c.builder.structs.len
		}
	}
}

// ==================== Stage 2 Checks ====================

pub fn (mut c SemanticChecker) check_stage2() bool {
	initial_errors := c.error_count()

	// Check each function body
	for ns in c.builder.namespaces {
		c.context.current_namespace = ns.id

		for fid in ns.functions {
			c.check_function_body(fid)
		}
	}

	return c.error_count() == initial_errors
}

fn (mut c SemanticChecker) check_function_body(fid FID) {
	func := c.builder.functions[fid]
	c.context.current_function = fid
	c.context.return_type = func.return_type

	// Enter function scope
	c.scope_enter()

	// Add function arguments to scope
	for arg in func.args {
		c.scope_define(arg.name, TypeInfo{
			typ:     arg.typ
			storage: arg.storage
			pos:     0
		})
	}

	// Check each basic block's instructions
	for bbid in func.bbs {
		c.check_basic_block(bbid, fid)
	}

	c.scope_exit()
	c.context.current_function = none
	c.context.return_type = none
}

fn (mut c SemanticChecker) check_basic_block(bbid BBID, fid FID) {
	bb := c.builder.basic_blocks[bbid]
	func := c.builder.functions[fid]

	// Add BB args to scope
	for arg in bb.args {
		c.scope_define(arg.name, TypeInfo{
			typ:     arg.typ
			storage: arg.storage
			pos:     0
		})
	}

	// Check each instruction
	for iid in bb.insts {
		c.check_instruction(func.insts[iid], fid)
	}
}

fn (mut c SemanticChecker) check_instruction(inst IRInstruction, fid FID) {
	func := c.builder.functions[fid]

	match inst {
		IRDefine {
			// Define adds a variable - check RHS type
			c.check_oid_valid(inst.value, fid)
		}
		IRTypedDefine {
			// Just a declaration, type already checked in stage 1
		}
		IRAssign {
			// Check LHS and RHS types match
			if left_type := c.get_rid_type(inst.result, fid) {
				if right_type := c.get_oid_type(inst.value, fid) {
					if !c.types_compatible(left_type, right_type) {
						c.add_error(.type_mismatch, 0,
							"cannot assign ${c.type_to_string(right_type)} to ${c.type_to_string(left_type)}")
					}
				}
			}
		}
		IRStore {
			// Store requires LHS to be a reference
			if left_type := c.get_rid_type(inst.result, fid) {
				if left_type !is IRRefType {
					c.add_error(.invalid_dereference, 0,
						"store target must be a reference type, got ${c.type_to_string(left_type)}")
				}
			}
		}
		IRBinaryOp {
			// Check operands are numeric for arithmetic
			if left_type := c.get_oid_type(inst.left, fid) {
				if !c.is_numeric_type(left_type) {
					c.add_error(.invalid_operation, 0,
						"binary operator requires numeric type, got ${c.type_to_string(left_type)}")
				}
			}
			if right_type := c.get_oid_type(inst.right, fid) {
				if !c.is_numeric_type(right_type) {
					c.add_error(.invalid_operation, 0,
						"binary operator requires numeric type, got ${c.type_to_string(right_type)}")
				}
			}
		}
		IRCall {
			// Check argument count
			called_func := c.builder.functions[inst.function]
			if inst.args.len != called_func.args.len {
				c.add_error(.wrong_argument_count, 0,
					"function '${called_func.name}' expects ${called_func.args.len} arguments, got ${inst.args.len}")
			} else {
				// Check argument types
				for i, arg_oid in inst.args {
					expected := called_func.args[i].typ
					actual := c.get_oid_type(arg_oid, fid) or { continue }

					if !c.types_compatible(expected, actual) {
						c.add_error(.wrong_argument_type, 0,
							"argument ${i + 1} of '${called_func.name}': expected ${c.type_to_string(expected)}, got ${c.type_to_string(actual)}")
					}
				}
			}
		}
		IRReturn {
			// Check return type matches function
			if ret_type := c.context.return_type {
				if value := inst.value {
					if actual := c.get_oid_type(value, fid) {
						if !c.types_compatible(ret_type, actual) {
							c.add_error(.type_mismatch, 0,
								"return type mismatch: expected ${c.type_to_string(ret_type)}, got ${c.type_to_string(actual)}")
						}
					}
				} else if ret_type != IRType(BuiltinType.void_t) {
					c.add_error(.missing_return, 0,
						"function must return a value of type ${c.type_to_string(ret_type)}")
				}
			}
		}
		IRDeref {
			// Check operand is a reference type
			if src_type := c.get_rid_type(inst.source, fid) {
				if src_type !is IRRefType {
					c.add_error(.invalid_dereference, 0,
						"cannot dereference non-reference type ${c.type_to_string(src_type)}")
				}
			}
		}
		IRRefInst {
			// Reference is always valid if source exists
			c.check_rid_valid(inst.source, fid)
		}
		else {
			// Other instructions don't need special checks
		}
	}
}

fn (mut c SemanticChecker) check_oid_valid(oid OID, fid FID) {
	match oid {
		CID {
			// Constants are always valid if they exist in the function
			func := c.builder.functions[fid]
			if int(oid) >= func.consts.len {
				c.add_error(.undefined_variable, 0, 'invalid constant reference')
			}
		}
		RID {
			c.check_rid_valid(oid, fid)
		}
	}
}

fn (mut c SemanticChecker) check_rid_valid(rid RID, fid FID) {
	func := c.builder.functions[fid]
	if int(rid) >= func.refs.len {
		c.add_error(.undefined_variable, 0, 'invalid reference')
	}
}

fn (c &SemanticChecker) get_rid_type(rid RID, fid FID) ?IRType {
	func := c.builder.functions[fid]
	if int(rid) >= func.refs.len {
		return none
	}
	return func.refs[rid].typ
}

fn (c &SemanticChecker) get_oid_type(oid OID, fid FID) ?IRType {
	func := c.builder.functions[fid]
	match oid {
		CID {
			if int(oid) >= func.consts.len {
				return none
			}
			cnst := func.consts[oid]
			return match cnst {
				IRIntConst { IRType(BuiltinType.int_t) }
				IRFloatConst { IRType(BuiltinType.float_t) }
				IRCharConst { IRType(BuiltinType.char_t) }
				IRStringConst { IRType(BuiltinType.string_t) }
				IRListConst { IRType(BuiltinType.list_t) }
				IRDictConst { IRType(BuiltinType.dict_t) }
				IRRangeConst { IRType(BuiltinType.list_t) } // Ranges are list-like
			}
		}
		RID {
			return c.get_rid_type(oid, fid)
		}
	}
}

fn (c &SemanticChecker) is_numeric_type(typ IRType) bool {
	return typ == IRType(BuiltinType.int_t) || typ == IRType(BuiltinType.float_t)
}

// ==================== Main Entry Point ====================

pub fn (mut c SemanticChecker) check() bool {
	// Source files are loaded by ErrorManager in main.v

	// Run both stages
	stage1_ok := c.check_stage1()
	stage2_ok := c.check_stage2()

	return stage1_ok && stage2_ok
}
