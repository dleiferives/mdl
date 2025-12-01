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

pub fn (mut s NSSolver) add_linked_tasks(block Block, parent_path string, id NSNodeId) []NSNodeId {
	path := parent_path
	if os.real_path(path) != path {
		panic('could not get node name of ${path} as it was not absolute. all calls must be absolute')
	}
	mut result := []NSNodeId{}
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
		}
	}
	s.tasks.push(NSTask(tasks))
	return result
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
	s.nodes[id].children = s.add_children_tasks(block, p, id)
	s.nodes[id].linked = s.add_linked_tasks(block, p, id)
	s.solved[id] = true
	return id
}

pub fn (mut s NSSolver) solve_file(path string) NSNodeId {
	p := os.real_path(path)
	mut id := s.nmap[p] or { s.new_node() }
	if s.solved[id] {
		return s.nmap[p] or { panic('${p} should be solved and therefore in the map...') }
	}
	s.nodes[id].name = get_file_node_name(p)
	s.nmap[p] = id
	// Resolve what the node we're going to create will be named
	block := get_file_node_block(p)
	s.nodes[id].children = s.add_children_tasks(block, p, id)
	s.nodes[id].linked = s.add_linked_tasks(block, p, id)
	s.solved[id] = true
	return id
}

pub fn (mut s NSSolver) solve(path string) ! {
	wd := os.getwd()
	s.solve_file(path)
	for !s.tasks.is_empty() {
		task := s.tasks.pop()!
		match task {
			NSLinkTask {
				for idx, link in task.list {
					id := task.ids[idx]
					s.nodes[id].name = link.name.name
					tpath := link.path.value
					os.chdir(task.parent_path)!
					println('tpath is ${tpath}')
					l_path := os.real_path(tpath)
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
