module main

fn main() {
	// Initialize error manager
	mut em := ErrorManager.new()

	// Get input files (for now using a test file)
	files := ['tests/codegen/arithmetic.mdl']

	// Load source files into error manager for error reporting
	for f in files {
		em.load_source(f)
	}

	// Create IR builder with error manager
	mut irb := IRBuilder{
		errors: &em
	}

	// Run lowering
	result := irb.lower(files) or {
		em.print_all()
		eprintln('Compilation failed.')
		exit(1)
	}

	// Check for errors
	if em.has_errors() || !result {
		em.print_all()
		exit(1)
	}

	// Print warnings if any
	if em.warning_count > 0 {
		em.print_all()
	}

	// Generate mcfunction output
	mut cg := Codegen.new(&irb, 'output', 'example')
	cg.generate() or {
		em.error(.internal_compiler_error, empty_location(), 'code generation failed: ${err}')
		em.print_all()
		exit(1)
	}
	println('Generated mcfunction files in output/')
}
