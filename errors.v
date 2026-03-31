module main

import os

// ==================== Source Location ====================

pub struct SourceLocation {
pub:
	file string
	pos  int // Byte offset
	len  int // Span length (for highlighting)
}

pub fn (loc SourceLocation) is_valid() bool {
	return loc.file.len > 0 && loc.pos >= 0
}

pub fn empty_location() SourceLocation {
	return SourceLocation{
		file: ''
		pos:  0
		len:  0
	}
}

// ==================== Error Severity ====================

pub enum Severity {
	error   // Compilation fails
	warning // Compilation continues, user should be aware
	hint    // Suggestion for improvement
}

// ==================== Error Kinds ====================

pub enum ErrorKind {
	// Lexer errors
	invalid_character
	unterminated_string
	invalid_escape_sequence
	invalid_number
	// Parser errors
	unexpected_token
	expected_identifier
	expected_type
	expected_expression
	expected_statement
	malformed_struct
	malformed_function
	malformed_namespace
	unclosed_delimiter
	// Type errors
	type_mismatch
	invalid_operation
	// Definition errors
	undefined_variable
	undefined_function
	undefined_struct
	undefined_field
	undefined_namespace
	// Declaration errors
	duplicate_definition
	shadowing_not_allowed
	// Reference errors
	invalid_reference
	invalid_dereference
	cannot_take_reference_of_constant
	// Structure errors
	missing_struct_field
	unknown_struct_field
	// Function errors
	wrong_argument_count
	wrong_argument_type
	missing_return
	invalid_return_type
	// Namespace errors
	namespace_collision
	namespace_not_found
	circular_import
	// IR lowering errors
	unsupported_statement_location
	invalid_basic_block
	empty_macro_chain
	cannot_resolve_type
	// File errors
	file_not_found
	file_read_error
	// Internal errors
	internal_compiler_error
	not_implemented
}

// ==================== Diagnostic Labels ====================

pub struct Label {
pub:
	location SourceLocation
	message  string
}

// ==================== Diagnostic ====================

pub struct Diagnostic {
pub:
	kind     ErrorKind
	severity Severity
	location SourceLocation
	message  string
	hint     string    // Optional suggestion
	labels   []Label   // Secondary spans
	notes    []string  // Additional context
}

// ==================== ANSI Colors ====================

struct Colors {
	reset   string = '\x1b[0m'
	bold    string = '\x1b[1m'
	dim     string = '\x1b[2m'
	red     string = '\x1b[91m'
	yellow  string = '\x1b[93m'
	cyan    string = '\x1b[96m'
	blue    string = '\x1b[94m'
	white   string = '\x1b[97m'
	magenta string = '\x1b[95m'
}

fn no_colors() Colors {
	return Colors{
		reset:   ''
		bold:    ''
		dim:     ''
		red:     ''
		yellow:  ''
		cyan:    ''
		blue:    ''
		white:   ''
		magenta: ''
	}
}

// ==================== Error Manager ====================

@[heap]
pub struct ErrorManager {
pub mut:
	diagnostics   []Diagnostic
	source_map    map[string]string // file path -> source content
	error_count   int
	warning_count int
	hint_count    int
	// Configuration
	max_errors  int  = 50   // Stop after this many errors
	colored     bool = true // Use ANSI colors
	show_hints  bool = true // Show hint-level diagnostics
}

pub fn ErrorManager.new() ErrorManager {
	return ErrorManager{
		diagnostics: []Diagnostic{}
		source_map:  map[string]string{}
	}
}

// Load source file content
pub fn (mut em ErrorManager) load_source(file string) {
	if file !in em.source_map {
		em.source_map[file] = os.read_file(file) or { '' }
	}
}

// Check if we should stop compilation
pub fn (em &ErrorManager) should_stop() bool {
	return em.error_count >= em.max_errors
}

pub fn (em &ErrorManager) has_errors() bool {
	return em.error_count > 0
}

fn (em &ErrorManager) colors() Colors {
	if em.colored {
		return Colors{}
	}
	return no_colors()
}

// ==================== Error Reporting Methods ====================

pub fn (mut em ErrorManager) report(severity Severity, kind ErrorKind, loc SourceLocation, msg string) {
	em.add_diagnostic(severity, kind, loc, msg, '', [], [])
}

pub fn (mut em ErrorManager) error(kind ErrorKind, loc SourceLocation, msg string) {
	em.add_diagnostic(.error, kind, loc, msg, '', [], [])
}

pub fn (mut em ErrorManager) error_with_hint(kind ErrorKind, loc SourceLocation, msg string, hint string) {
	em.add_diagnostic(.error, kind, loc, msg, hint, [], [])
}

pub fn (mut em ErrorManager) error_with_labels(kind ErrorKind, loc SourceLocation, msg string, labels []Label) {
	em.add_diagnostic(.error, kind, loc, msg, '', labels, [])
}

pub fn (mut em ErrorManager) error_full(kind ErrorKind, loc SourceLocation, msg string, hint string, labels []Label, notes []string) {
	em.add_diagnostic(.error, kind, loc, msg, hint, labels, notes)
}

pub fn (mut em ErrorManager) warning(kind ErrorKind, loc SourceLocation, msg string) {
	em.add_diagnostic(.warning, kind, loc, msg, '', [], [])
}

pub fn (mut em ErrorManager) warning_with_hint(kind ErrorKind, loc SourceLocation, msg string, hint string) {
	em.add_diagnostic(.warning, kind, loc, msg, hint, [], [])
}

pub fn (mut em ErrorManager) hint_msg(kind ErrorKind, loc SourceLocation, msg string) {
	em.add_diagnostic(.hint, kind, loc, msg, '', [], [])
}

fn (mut em ErrorManager) add_diagnostic(severity Severity, kind ErrorKind, loc SourceLocation, msg string, hint string, labels []Label, notes []string) {
	match severity {
		.error { em.error_count++ }
		.warning { em.warning_count++ }
		.hint { em.hint_count++ }
	}

	em.diagnostics << Diagnostic{
		kind:     kind
		severity: severity
		location: loc
		message:  msg
		hint:     hint
		labels:   labels
		notes:    notes
	}
}

// ==================== Location Utilities ====================

// Convert byte position to line and column (1-indexed)
pub fn (em &ErrorManager) pos_to_location(file string, pos int) (int, int) {
	src := em.source_map[file] or { return 1, 1 }
	mut line := 1
	mut col := 1
	for i := 0; i < pos && i < src.len; i++ {
		if src[i] == `\n` {
			line++
			col = 1
		} else {
			col++
		}
	}
	return line, col
}

// Get the source line containing a position
fn (em &ErrorManager) get_source_line(file string, pos int) (string, int) {
	src := em.source_map[file] or { return '', 0 }
	if pos >= src.len {
		return '', 0
	}

	// Find start of line
	mut start := pos
	for start > 0 && src[start - 1] != `\n` {
		start--
	}

	// Find end of line
	mut end := pos
	for end < src.len && src[end] != `\n` {
		end++
	}

	return src[start..end], start
}

// ==================== Pretty Printing ====================

pub fn (em &ErrorManager) print_all() {
	for diag in em.diagnostics {
		if diag.severity == .hint && !em.show_hints {
			continue
		}
		em.print_diagnostic(diag)
	}
	em.print_summary()
}

fn (em &ErrorManager) print_diagnostic(diag Diagnostic) {
	c := em.colors()

	// Get line and column
	mut line := 1
	mut col := 1
	if diag.location.is_valid() {
		line, col = em.pos_to_location(diag.location.file, diag.location.pos)
	}

	// Header line: error[kind]: message
	severity_str, severity_color := match diag.severity {
		.error { 'error', c.red }
		.warning { 'warning', c.yellow }
		.hint { 'hint', c.cyan }
	}

	println('${c.bold}${severity_color}${severity_str}${c.reset}${c.bold}[${diag.kind}]${c.reset}: ${diag.message}')

	// Location line: --> file:line:col
	if diag.location.is_valid() {
		println('  ${c.blue}-->${c.reset} ${diag.location.file}:${line}:${col}')

		// Source context
		em.print_source_context(diag, line, col, severity_color)
	}

	// Print labels (secondary spans)
	for label in diag.labels {
		if label.location.is_valid() {
			l_line, l_col := em.pos_to_location(label.location.file, label.location.pos)
			println('  ${c.blue}:${c.reset}')
			println('  ${c.blue}= note${c.reset}: ${label.message}')
			println('  ${c.blue}-->${c.reset} ${label.location.file}:${l_line}:${l_col}')

			// Print source context for label
			em.print_label_context(label, l_line, l_col)
		}
	}

	// Print hint
	if diag.hint.len > 0 {
		println('  ${c.blue}=${c.reset} ${c.cyan}hint${c.reset}: ${diag.hint}')
	}

	// Print notes
	for note in diag.notes {
		println('  ${c.blue}= note${c.reset}: ${note}')
	}

	println('')
}

fn (em &ErrorManager) print_source_context(diag Diagnostic, line int, col int, severity_color string) {
	c := em.colors()
	src_line, _ := em.get_source_line(diag.location.file, diag.location.pos)
	if src_line.len == 0 {
		return
	}

	line_num_width := max_int('${line}'.len, 3)

	// Empty line with gutter
	gutter := ' '.repeat(line_num_width)
	println('${c.blue}${gutter} |${c.reset}')

	// Source line with line number
	line_str := str_rjust('${line}', line_num_width)
	println('${c.blue}${line_str} |${c.reset} ${src_line}')

	// Underline with carets
	span_len := if diag.location.len > 0 { diag.location.len } else { 1 }
	padding := ' '.repeat(col - 1)
	underline := '^'.repeat(span_len)
	println('${c.blue}${gutter} |${c.reset} ${padding}${severity_color}${underline}${c.reset}')
}

fn (em &ErrorManager) print_label_context(label Label, line int, col int) {
	c := em.colors()
	src_line, _ := em.get_source_line(label.location.file, label.location.pos)
	if src_line.len == 0 {
		return
	}

	line_num_width := max_int('${line}'.len, 3)
	gutter := ' '.repeat(line_num_width)

	println('${c.blue}${gutter} |${c.reset}')
	line_str := str_rjust('${line}', line_num_width)
	println('${c.blue}${line_str} |${c.reset} ${src_line}')

	span_len := if label.location.len > 0 { label.location.len } else { 1 }
	padding := ' '.repeat(col - 1)
	underline := '-'.repeat(span_len)
	println('${c.blue}${gutter} |${c.reset} ${padding}${c.blue}${underline}${c.reset}')
}

fn (em &ErrorManager) print_summary() {
	c := em.colors()
	if em.error_count > 0 {
		mut parts := []string{}
		if em.error_count > 0 {
			s := if em.error_count == 1 { '' } else { 's' }
			parts << '${em.error_count} error${s}'
		}
		if em.warning_count > 0 {
			s := if em.warning_count == 1 { '' } else { 's' }
			parts << '${em.warning_count} warning${s}'
		}
		println('${c.bold}${c.red}error${c.reset}: could not compile due to ${parts.join(', ')}')
	} else if em.warning_count > 0 {
		s := if em.warning_count == 1 { '' } else { 's' }
		println('${c.bold}${c.yellow}warning${c.reset}: ${em.warning_count} warning${s} generated')
	}
}

// ==================== Helpers ====================

fn max_int(a int, b int) int {
	return if a > b { a } else { b }
}

fn str_rjust(s string, width int) string {
	if s.len >= width {
		return s
	}
	return ' '.repeat(width - s.len) + s
}
