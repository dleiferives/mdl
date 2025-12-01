import os
import datatypes

type NSNodeId = int

@[heap]
pub struct NSNode {
pub mut:
	id          int
	name        string
	children    []NSNodeId
	linked      []NSNodeId
	linked_name []string
	valid       bool = true
	block       Block
}

@[heap]
pub struct NSChildTask {
pub mut:
	parent      NSNodeId
	parent_path string
	list        []NamespaceDefinition
	ids         []NSNodeId
}

@[heap]
pub struct NSLinkTask {
pub mut:
	parent      NSNodeId
	parent_path string
	list        []NamespaceImport
	ids         []NSNodeId
}

type NSTask = NSChildTask | NSLinkTask

pub struct NSSolver {
pub mut:
	nodes  []NSNode
	nmap   map[string]NSNodeId // maps path to node
	solved map[NSNodeId]bool
	tasks  datatypes.Stack[NSTask]
}

pub fn (mut s NSSolver) new_node() NSNodeId {
	mut new := NSNode{}
	new.id = s.nodes.len
	s.nodes << new
	s.solved[new.id] = false
	return new.id
}

pub fn (mut s NSSolver) add_children_tasks(block Block, parent_path string, id NSNodeId) []NSNodeId {
	path := parent_path
	if os.real_path(path) != path {
		panic('could not get node name of ${path} as it was not absolute. all calls must be absolute')
	}
	mut result := []NSNodeId{}
	mut tasks := NSChildTask{
		parent:      id
		parent_path: parent_path
	}

	ast := block.stmts
	for stmt in ast {
		if stmt is NamespaceDefinition {
			cid := s.new_node()
			tasks.list << stmt
			tasks.ids << cid
			result << cid
		}
	}
	s.tasks.push(NSTask(tasks))
	return result
}

pub fn (mut s NSSolver) add_linked_tasks(block Block, parent_path string, id NSNodeId) ([]NSNodeId, []string) {
	path := parent_path
	if os.real_path(path) != path {
		panic('could not get node name of ${path} as it was not absolute. all calls must be absolute')
	}
	mut result := []NSNodeId{}
	mut nresult := []string{}
	mut tasks := NSLinkTask{
		parent:      id
		parent_path: parent_path
	}

	ast := block.stmts
	for stmt in ast {
		if stmt is NamespaceImport {
			cid := s.new_node()
			tasks.list << stmt
			tasks.ids << cid
			result << cid
			nresult << stmt.name.name
		}
	}
	s.tasks.push(NSTask(tasks))
	return result, nresult
}

// path must be absolute
pub fn get_file_node_name(path string) string {
	if os.real_path(path) != path {
		panic('could not get node name of ${path} as it was not absolute. all calls must be absolute')
	}

	ast := parse_file(path)
	for stmt in ast {
		if stmt is NamespaceAlias {
			return stmt.name.name
		}
	}
	_, result, _ := os.split_path(path)
	return result
}

pub fn get_file_node_block(path string) Block {
	if os.real_path(path) != path {
		panic('could not get node name of ${path} as it was not absolute. all calls must be absolute')
	}

	ast := parse_file(path)
	for stmt in ast {
		if stmt is NamespaceAlias {
			return stmt.block
		}
	}
	return Block{
		Node:  Node{}
		stmts: ast
	}
}

pub fn (mut s NSSolver) solve_child(child NamespaceDefinition, path string, id NSNodeId) NSNodeId {
	p := os.real_path(path)
	if s.solved[id] {
		return id
	}
	s.nodes[id].name = child.name.name
	// Resolve what the node we're going to create will be named
	block := child.block
	s.nodes[id].children << s.add_children_tasks(block, p, id)
	linked, names := s.add_linked_tasks(block, p, id)
	s.nodes[id].linked << linked
	s.nodes[id].linked_name << names
	s.nodes[id].block = block
	s.solved[id] = true
	return id
}

pub fn (mut s NSSolver) solve_file(path string) NSNodeId {
	println('solving file ${path}')
	p := os.real_path(path)
	mut id := s.nmap[p] or { s.new_node() }
	println('working on node ${id}')
	if s.solved[id] {
		return s.nmap[p] or { panic('${p} should be solved and therefore in the map...') }
	}
	s.nodes[id].name = get_file_node_name(p)
	println('nodes name is ${s.nodes[id].name}')
	s.nmap[p] = id
	// Resolve what the node we're going to create will be named
	block := get_file_node_block(p)
	s.nodes[id].children << s.add_children_tasks(block, p, id)
	linked, names := s.add_linked_tasks(block, p, id)
	s.nodes[id].linked << linked
	s.nodes[id].linked_name << names
	s.nodes[id].block = block
	s.solved[id] = true
	return id
}

pub fn (mut s NSSolver) solve(path string) ! {
	println('starting solve of ${path}')
	wd := os.getwd()
	s.solve_file(path)
	for !s.tasks.is_empty() {
		println('solving task')
		task := s.tasks.pop()!
		match task {
			NSLinkTask {
				for idx, link in task.list {
					id := task.ids[idx]
					s.nodes[id].name = link.name.name
					println('task nodes name is ${s.nodes[id].name}')
					tpath := link.path.value[1..link.path.value.len - 1]
					println('task path is ${tpath}')
					println('parent is ${task.parent_path}')
					pp, _, _ := os.split_path(task.parent_path)
					os.chdir(pp)!
					l_path := os.real_path(tpath)
					nid := s.nmap[l_path] or { id }
					s.nmap[l_path] = nid
					if nid != id {
						println('${nid} != ${id}')
						for lid, lv in s.nodes[task.parent].linked {
							if lv == id {
								s.nodes[task.parent].linked[lid] = nid
							}
						}
						assert s.solved[id] == false
						s.nodes[id].valid = false
					}
					if s.solved[nid] {
						println('${nid} already solved')
						continue
					}
					s.solve_file(l_path)
				}
			}
			NSChildTask {
				for idx, child in task.list {
					id := task.ids[idx]
					s.solve_child(child, task.parent_path, id)
				}
			}
		}
	}
	os.chdir(wd)!
}

// TODO: add imports so that things in same file can use each other.
pub fn (mut s NSSolver) verify_node_legal(n NSNode, mut valid map[NSNodeId]bool) bool {
	mut child_defs := []string{}
	child_defs << n.name
	for c in n.children {
		if s.nodes[c].name in child_defs {
			return false
		}
		child_defs << s.nodes[c].name
		valid[c] = true
	}
	for l in n.linked_name {
		if l in child_defs {
			return false
		}
		child_defs << l
	}
	valid[n.id] = true
	return true
}

pub fn (mut s NSSolver) verify_legal() bool {
	// Make sure that there is no renaming within a heirarchy
	// TODO: there will be edge cases with importing things from other namespaces

	// One of the edge cases that I'm thinking about is where we have multiple
	// files that say they are the same namespace, but then we import them into
	// another file as a different namespace. I don't really want to have to
	// handle that? Because there is a very rigid structure that we are using to
	// make this happen at the moment, though this could be resolved if we
	// gaurentee that it will be the child of the other namespace. Therefore
	// this is something that we should handle.
	//
	// For the moment I'm going to make it so that it becomes the child if it
	// can. However, there should be an option for making this not happen so
	// that mutliple things can use the same library code.
	//
	// That would really be something for later when dealing with interfacing of
	// projects.
	//
	// I'm just going to not deal with it actually. Just like if there's
	// colision... that's someone elses problem. Or really just my problem for
	// later.
	//
	// Let's just throw a warning.
	//
	//
	// TODO: add import
	mut tlns := []string{}
	tll: for n in s.nodes {
		if !n.valid {
			continue
		}
		for n2 in s.nodes {
			if !n2.valid {
				continue
			}
			if n.id in n2.children {
				continue tll
			}
		}
		if n.name in tlns {
			println('Warning: found namespace duplicate ${n.name}')
		}
		tlns << n.name
	}
	mut valid := map[NSNodeId]bool{}
	for n in s.nodes {
		valid[n.id] = valid[n.id] or { false }
		if !s.verify_node_legal(n, mut valid) {
			return false
		}
	}
	// println(valid)
	return true
}
