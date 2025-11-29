module main

// TODO: todoit lol

// fn main() {
// 	example()
// 	print('\n')
// }

// fn example() {
// 	str := 'reg a := 10;
// data something := 20;
// reg b := a + something;
// '
// 	res := parse(str)
// 	print(res)
// 	mut fun := Function{
// 		name: 'test'
// 	}
// 	fun.init()

// 	generate_expr(res[0], res, mut &fun)
// 	generate_expr(res[1], res, mut &fun)
// 	generate_expr(res[2], res, mut &fun)
// 	print(fun)
// }

pub fn generate_expr(node Expr, ast []Expr, mut fun Function) Ref {
	match node {
		Identifier {
			// Search Globals
			// TODO:
			// Search namespace
			// TODO:
			// Search Functions
			// TODO:
			// Search Args
			// TODO:
			// Search function scope
			for reg in fun.regs.list {
				if reg.name == node.name {
					return reg.to_ref(.local)
				}
			}

			// Search blockscope
			// TODO:
			return Ref{
				kind: .illegal
			}
		}
		Integer {
			return Ref{
				kind: .immediate
				imm:  node.value
			}
		}
		Define {
			print(fun.name)
			// if not a global
			// if not a function name
			// if not a paramater
			// if not already defined in this scope

			// Expr
			ref_expr := generate_expr(node.value, ast, mut fun)

			mut src_ref := generate_expr(Expr(node.name), ast, mut fun)
			mut inst := fun.next_instruction()
			// set
			mut reg := fun.next_register()
			reg.name = node.name.name
			// TODO: type
			reg.source = node.source.to_ir()
			reg.inst_id = inst.id
			src_ref = fun.add_register(reg, .local)

			// TODO: basic block
			inst.kind = Set{
				dst: src_ref
				src: ref_expr
			}
			fun.add_instruction(inst)
			return inst.kind.dst
		}
		BinaryExpr {
			match node.operator {
				.plus {
					lhs_ref := generate_expr(node.left, ast, mut fun)
					rhs_ref := generate_expr(node.right, ast, mut fun)
					mut reg := fun.next_register()
					reg.source = .real
					mut inst := fun.next_instruction()
					reg.inst_id = inst.id
					inst.kind = BinaryOp{
						kind: .add
						dst:  fun.add_register(reg, .local)
						lhs:  lhs_ref
						rhs:  rhs_ref
					}
					fun.add_instruction(inst)
					return inst.kind.dst
				}
				else {
					panic('In BinaryExpr ${node.operator} not yet handled in ${node}')
				}
			}
		}
		else {
			panic('no yet implemented ${node} type')
		}
	}
	return Ref{
		kind: .illegal
	}
}

struct LookupTable[V] {
mut:
	list   []V
	get_id ?fn (V) int
	len    int
}

fn (mut t LookupTable[V]) lookup(key int) ?int {
	gid := t.get_id or { return none }
	for i, ent in t.list {
		item := gid(ent)
		if item == key {
			return i
		}
	}
	return none
}

fn (mut t LookupTable[V]) get(key int) V {
	return t.list[key]
}

fn (mut t LookupTable[V]) add(v V) int {
	t.list << v
	t.len += 1
	return t.len - 1
}

fn reg_get_id(r Register) int {
	return r.id
}

fn inst_get_id(i Instruction) int {
	return i.id
}

struct Function {
	name string
mut:
	regs  LookupTable[Register]
	insts LookupTable[Instruction]
}

fn (mut f Function) init() {
	f.regs.get_id = reg_get_id
	f.insts.get_id = inst_get_id
}

fn (mut f Function) next_register() Register {
	return Register{
		id: f.regs.len
	}
}

fn (mut f Function) next_instruction() Instruction {
	return Instruction{
		id: f.insts.len
	}
}

fn (mut f Function) add_register(r Register, kind RefKind) Ref {
	if r.id != f.regs.len {
		panic('${r} not last id in ${f}')
	}
	f.regs.add(r)
	// TODO establish kind...
	return Ref{r.id, r.name, kind, r.type, r.source, 0}
}

fn (mut f Function) add_instruction(inst Instruction) int {
	if inst.id != f.insts.len {
		panic('${inst} not last id in ${f}')
	}
	f.insts.add(inst)
	if f.regs.get(inst.kind.dst.id).inst_id != inst.id {
		panic('dst register of instruction does not point to this instruction ${inst}\n ${f}')
	}
	return inst.id
}

enum IrSource {
	effemeral
	register
	data
	real
}

fn (v ValueType) to_ir() IrSource {
	match v {
		.effemeral { return IrSource.effemeral }
		.register { return IrSource.register }
		.data { return IrSource.data }
	}
	return .real
}

struct Register {
mut:
	id      int      // the ID of the register in the register table, should be itself
	name    string   // The name of the register
	type    Type     // The type of the register
	source  IrSource // The type of the register
	inst_id int      // The instruction the defines the register
	bb_id   int      // the basic block its in
}

fn (r Register) to_ref(kind RefKind) Ref {
	return Ref{r.id, r.name, kind, r.type, r.source, 0}
}

enum RefKind {
	illegal
	local
	immediate
	global
	namespace
	paramater
}

// Reference to a register
struct Ref {
mut:
	id     int      // Id of the register in the register table
	name   string   // name of the reference
	kind   RefKind  // the kind the reference is (local global function etc)
	type   Type     // the type
	source IrSource // The type of the register
	imm    int
}

enum BinaryOpKind {
	add
	sub
	// mul floor div swap lt gt eq mod
}

type InstructionKind = BinaryOp | Set

struct Instruction {
mut:
	id   int
	kind InstructionKind
}

struct BinaryOp {
mut:
	kind BinaryOpKind
	dst  Ref
	lhs  Ref
	rhs  Ref
}

struct Set {
mut:
	dst Ref
	src Ref
}
