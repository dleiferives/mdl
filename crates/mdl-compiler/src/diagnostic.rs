//! Shared compiler diagnostics.

use std::error::Error;
use std::fmt;

use crate::source::{OriginId, SourceContext, SourceLine, Span};

/// One diagnostic location with optional location-specific context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticLabel {
    origin: OriginId,
    message: Option<String>,
}

impl DiagnosticLabel {
    /// Creates an unlabeled diagnostic location.
    #[must_use]
    pub const fn new(origin: OriginId) -> Self {
        Self {
            origin,
            message: None,
        }
    }

    /// Attaches a message explaining this location's role in the diagnostic.
    #[must_use]
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// Returns this label's provenance.
    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }

    /// Returns the location-specific message, when present.
    #[must_use]
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }
}

/// One independently actionable compiler finding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    code: &'static str,
    message: String,
    primary: DiagnosticLabel,
    supporting: Vec<DiagnosticLabel>,
    notes: Vec<String>,
}

impl Diagnostic {
    pub(crate) fn new(code: &'static str, message: impl Into<String>, origin: OriginId) -> Self {
        Self {
            code,
            message: message.into(),
            primary: DiagnosticLabel::new(origin),
            supporting: vec![],
            notes: vec![],
        }
    }

    /// Attaches a message to the primary location.
    #[must_use]
    pub fn with_primary_label(mut self, message: impl Into<String>) -> Self {
        self.primary.message = Some(message.into());
        self
    }

    /// Appends one supporting location in deterministic display order.
    #[must_use]
    pub fn with_supporting_label(mut self, label: DiagnosticLabel) -> Self {
        self.supporting.push(label);
        self
    }

    /// Appends one explanatory note in deterministic display order.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    /// Returns the stable diagnostic code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// Returns the human-readable diagnostic message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the primary diagnostic location and its optional message.
    #[must_use]
    pub const fn primary_label(&self) -> &DiagnosticLabel {
        &self.primary
    }

    /// Returns supporting locations in producer-defined order.
    #[must_use]
    pub fn supporting_labels(&self) -> &[DiagnosticLabel] {
        &self.supporting
    }

    /// Returns explanatory notes in producer-defined order.
    #[must_use]
    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    /// Returns the best available provenance for the finding.
    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.primary.origin
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}: {} ({:?})",
            self.code,
            self.message,
            self.origin()
        )
    }
}

/// Accumulated compiler diagnostics in deterministic production order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostics {
    findings: Vec<Diagnostic>,
}

impl Diagnostics {
    pub(crate) fn from_findings(findings: Vec<Diagnostic>) -> Option<Self> {
        (!findings.is_empty()).then_some(Self { findings })
    }

    pub(crate) fn into_findings(self) -> Vec<Diagnostic> {
        self.findings
    }

    /// Returns all findings in deterministic production order.
    #[must_use]
    pub fn findings(&self) -> &[Diagnostic] {
        &self.findings
    }

    /// Returns the number of findings.
    #[must_use]
    pub fn len(&self) -> usize {
        self.findings.len()
    }

    /// Returns whether no findings are present.
    ///
    /// A constructed [`Diagnostics`] value is always nonempty. This method remains
    /// useful to callers inspecting a borrowed value and preserves the original
    /// Core verifier API.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.findings.is_empty()
    }

    /// Returns whether at least one finding has the given stable code.
    #[must_use]
    pub fn contains_code(&self, code: &str) -> bool {
        self.findings.iter().any(|finding| finding.code == code)
    }
}

impl fmt::Display for Diagnostics {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, finding) in self.findings.iter().enumerate() {
            if index != 0 {
                formatter.write_str("\n")?;
            }
            write!(formatter, "{finding}")?;
        }
        Ok(())
    }
}

impl Error for Diagnostics {}

/// Renders one diagnostic with borrowed source context and no I/O or terminal state.
///
/// Locations use one-based line numbers and one-based UTF-8 byte columns derived
/// from the zero-based [`crate::source::SourcePosition`]. No terminal-width
/// alignment is promised.
/// Unknown or unresolvable provenance is rendered explicitly as `<unknown>`.
#[must_use]
pub fn render_diagnostic(diagnostic: &Diagnostic, sources: &SourceContext) -> String {
    let mut lines = vec![format!(
        "error[{}]: {}",
        diagnostic.code(),
        diagnostic.message()
    )];
    render_label(&mut lines, diagnostic.primary_label(), sources, true);
    for label in diagnostic.supporting_labels() {
        render_label(&mut lines, label, sources, false);
    }
    for note in diagnostic.notes() {
        lines.push(format!("  = note: {note}"));
    }
    lines.join("\n")
}

/// Renders diagnostics in deterministic production order without performing I/O.
///
/// Findings are separated by one empty line. The returned text has no trailing
/// newline.
#[must_use]
pub fn render_diagnostics(diagnostics: &Diagnostics, sources: &SourceContext) -> String {
    diagnostics
        .findings()
        .iter()
        .map(|diagnostic| render_diagnostic(diagnostic, sources))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_label(
    output: &mut Vec<String>,
    label: &DiagnosticLabel,
    sources: &SourceContext,
    primary: bool,
) {
    let arrow = if primary { "-->" } else { ":::" };
    let Some(span) = sources.resolve_origin_span(label.origin()) else {
        render_unknown_label(output, label, arrow, primary);
        return;
    };
    let Some(file) = sources.files().get(span.file()) else {
        render_unknown_label(output, label, arrow, primary);
        return;
    };
    let Ok(line) = sources.files().line_at(span.file(), span.start()) else {
        render_unknown_label(output, label, arrow, primary);
        return;
    };
    let Ok(position) = sources.files().resolve_position(span.file(), span.start()) else {
        render_unknown_label(output, label, arrow, primary);
        return;
    };
    let Some(marker) = render_source_marker(line, span, primary) else {
        render_unknown_label(output, label, arrow, primary);
        return;
    };

    let line_number = u64::from(line.number()) + 1;
    let column = u64::from(position.byte_column) + 1;
    let line_width = line_number.to_string().len();
    output.push(format!(" {arrow} {}:{line_number}:{column}", file.name()));
    output.push(format!("{:>line_width$} |", ""));
    output.push(format!("{line_number:>line_width$} | {}", line.text()));
    let message = label
        .message()
        .map_or_else(String::new, |message| format!(" {message}"));
    output.push(format!(
        "{:>line_width$} | {}{}{}",
        "", marker.indent, marker.underline, message
    ));
}

fn render_unknown_label(
    output: &mut Vec<String>,
    label: &DiagnosticLabel,
    arrow: &str,
    primary: bool,
) {
    output.push(format!(" {arrow} <unknown>"));
    if let Some(message) = label.message() {
        let role = if primary { "primary" } else { "supporting" };
        output.push(format!("  = {role}: {message}"));
    }
}

struct RenderedMarker {
    indent: String,
    underline: String,
}

fn render_source_marker(line: SourceLine<'_>, span: Span, primary: bool) -> Option<RenderedMarker> {
    let line_span = line.span();
    let visible_start = span.start().clamp(line_span.start(), line_span.end());
    let visible_end = span.end().clamp(visible_start, line_span.end());
    let start = usize::try_from(visible_start.checked_sub(line_span.start())?).ok()?;
    let end = usize::try_from(visible_end.checked_sub(line_span.start())?).ok()?;
    let prefix = line.text().get(..start)?;
    let covered = line.text().get(start..end)?;
    let indent = prefix
        .chars()
        .map(|character| if character == '\t' { '\t' } else { ' ' })
        .collect();
    let marker = if primary { '^' } else { '-' };
    let underline = std::iter::repeat_n(marker, covered.chars().count().max(1)).collect();
    Some(RenderedMarker { indent, underline })
}

#[cfg(test)]
mod tests {
    use super::{Diagnostic, DiagnosticLabel, Diagnostics, render_diagnostic, render_diagnostics};
    use crate::source::{Origin, OriginId, SourceContext};

    #[test]
    fn core_reexports_the_shared_diagnostics_type() {
        fn accepts_shared(_: Option<Diagnostics>) {}

        let through_core: Option<crate::ir::core::Diagnostics> = None;
        accepts_shared(through_core);
    }

    #[test]
    fn labels_and_notes_extend_the_existing_api_without_changing_legacy_display() {
        let primary = OriginId::UNKNOWN;
        let mut foreign = SourceContext::new();
        let supporting = foreign.add_origin(Origin::Unknown).unwrap();
        let diagnostic = Diagnostic::new("front.example", "example failure", primary)
            .with_primary_label("primary context")
            .with_supporting_label(
                DiagnosticLabel::new(supporting).with_message("supporting context"),
            )
            .with_supporting_label(DiagnosticLabel::new(primary))
            .with_note("first note")
            .with_note("second note");

        assert_eq!(diagnostic.code(), "front.example");
        assert_eq!(diagnostic.message(), "example failure");
        assert_eq!(diagnostic.origin(), primary);
        assert_eq!(diagnostic.primary_label().origin(), primary);
        assert_eq!(
            diagnostic.primary_label().message(),
            Some("primary context")
        );
        assert_eq!(diagnostic.supporting_labels().len(), 2);
        assert_eq!(
            diagnostic.supporting_labels()[0].message(),
            Some("supporting context")
        );
        assert_eq!(diagnostic.supporting_labels()[1].message(), None);
        assert_eq!(diagnostic.notes(), ["first note", "second note"]);
        assert_eq!(diagnostic, diagnostic.clone());
        assert_eq!(
            diagnostic.to_string(),
            format!("front.example: example failure ({primary:?})")
        );
    }

    #[test]
    fn renderer_uses_utf8_byte_columns_and_preserves_label_and_note_order() {
        let mut sources = SourceContext::new();
        let text = "fn main() {\n    const café: Int32 = true;\n}\n";
        let file = sources.add_file("main.mdl", text).unwrap();
        let int_start = u32::try_from(text.find("Int32").unwrap()).unwrap();
        let bool_start = u32::try_from(text.find("true").unwrap()).unwrap();
        let expected = sources
            .add_origin(Origin::Source(
                sources.span(file, int_start, int_start + 5).unwrap(),
            ))
            .unwrap();
        let actual = sources
            .add_origin(Origin::Source(
                sources.span(file, bool_start, bool_start + 4).unwrap(),
            ))
            .unwrap();
        let diagnostic =
            Diagnostic::new("front.type-mismatch", "expected Int32, found Bool", actual)
                .with_primary_label("has type Bool")
                .with_supporting_label(
                    DiagnosticLabel::new(expected).with_message("declared as Int32 here"),
                )
                .with_note("conversions are explicit");

        let expected_render = [
            "error[front.type-mismatch]: expected Int32, found Bool".to_owned(),
            " --> main.mdl:2:26".to_owned(),
            "  |".to_owned(),
            "2 |     const café: Int32 = true;".to_owned(),
            format!("  | {}^^^^ has type Bool", " ".repeat(24)),
            " ::: main.mdl:2:18".to_owned(),
            "  |".to_owned(),
            "2 |     const café: Int32 = true;".to_owned(),
            format!("  | {}----- declared as Int32 here", " ".repeat(16)),
            "  = note: conversions are explicit".to_owned(),
        ]
        .join("\n");
        assert_eq!(render_diagnostic(&diagnostic, &sources), expected_render);
        assert_eq!(render_diagnostic(&diagnostic, &sources), expected_render);
    }

    #[test]
    fn renderer_handles_empty_eof_and_unknown_locations() {
        let mut sources = SourceContext::new();
        let file = sources.add_file("eof.mdl", "é\n").unwrap();
        let eof = sources
            .add_origin(Origin::Source(sources.span(file, 3, 3).unwrap()))
            .unwrap();
        let eof_diagnostic =
            Diagnostic::new("parse.expected", "expected an item", eof).with_primary_label("EOF");
        assert_eq!(
            render_diagnostic(&eof_diagnostic, &sources),
            "error[parse.expected]: expected an item\n --> eof.mdl:2:1\n  |\n2 | \n  | ^ EOF"
        );

        let crlf_file = sources.add_file("crlf.mdl", "x\r\n").unwrap();
        let line_feed = sources
            .add_origin(Origin::Source(sources.span(crlf_file, 2, 3).unwrap()))
            .unwrap();
        let crlf_diagnostic =
            Diagnostic::new("parse.line-ending", "unexpected line ending", line_feed)
                .with_primary_label("line ending");
        assert_eq!(
            render_diagnostic(&crlf_diagnostic, &sources),
            "error[parse.line-ending]: unexpected line ending\n --> crlf.mdl:1:3\n  |\n1 | x\n  |  ^ line ending"
        );

        let mut foreign = SourceContext::new();
        let mut absent = OriginId::UNKNOWN;
        for _ in 0..sources.origin_count() {
            absent = foreign.add_origin(Origin::Unknown).unwrap();
        }
        let unknown = Diagnostic::new("front.unknown", "location unavailable", OriginId::UNKNOWN)
            .with_primary_label("primary context")
            .with_supporting_label(DiagnosticLabel::new(absent).with_message("supporting context"));
        assert_eq!(
            render_diagnostic(&unknown, &sources),
            "error[front.unknown]: location unavailable\n --> <unknown>\n  = primary: primary context\n ::: <unknown>\n  = supporting: supporting context"
        );
    }

    #[test]
    fn renderer_aligns_blank_and_marker_gutters_for_multi_digit_lines() {
        let mut sources = SourceContext::new();
        let text = format!("{}target\n", "ignored\n".repeat(9));
        let file = sources.add_file("ten.mdl", text.clone()).unwrap();
        let start = u32::try_from(text.find("target").unwrap()).unwrap();
        let origin = sources
            .add_origin(Origin::Source(
                sources.span(file, start, start + 6).unwrap(),
            ))
            .unwrap();
        let diagnostic =
            Diagnostic::new("front.tenth-line", "line ten", origin).with_primary_label("aligned");

        assert_eq!(
            render_diagnostic(&diagnostic, &sources),
            "error[front.tenth-line]: line ten\n --> ten.mdl:10:1\n   |\n10 | target\n   | ^^^^^^ aligned"
        );
    }

    #[test]
    fn renderer_resolves_call_site_and_fused_provenance() {
        let mut sources = SourceContext::new();
        let definition_file = sources.add_file("callee.mdl", "callee").unwrap();
        let invocation_file = sources.add_file("caller.mdl", "caller").unwrap();
        let definition_origin = sources
            .add_origin(Origin::Source(sources.span(definition_file, 0, 6).unwrap()))
            .unwrap();
        let invocation_origin = sources
            .add_origin(Origin::Source(sources.span(invocation_file, 0, 6).unwrap()))
            .unwrap();
        let call_site = sources
            .add_origin(Origin::CallSite {
                callee: definition_origin,
                caller: invocation_origin,
            })
            .unwrap();
        let fused = sources
            .add_origin(Origin::Fused {
                inputs: vec![OriginId::UNKNOWN, definition_origin],
                reason: Some("combined".into()),
            })
            .unwrap();
        let diagnostic = Diagnostic::new("front.composite", "composite provenance", call_site)
            .with_supporting_label(DiagnosticLabel::new(fused));
        let rendered = render_diagnostic(&diagnostic, &sources);

        assert!(
            rendered
                .starts_with("error[front.composite]: composite provenance\n --> caller.mdl:1:1")
        );
        assert!(rendered.contains("\n ::: callee.mdl:1:1\n"));
    }

    #[test]
    fn diagnostics_renderer_preserves_finding_order_without_a_trailing_newline() {
        let sources = SourceContext::new();
        let diagnostics = Diagnostics::from_findings(vec![
            Diagnostic::new("front.first", "first", OriginId::UNKNOWN),
            Diagnostic::new("front.second", "second", OriginId::UNKNOWN).with_note("detail"),
        ])
        .unwrap();

        assert_eq!(
            render_diagnostics(&diagnostics, &sources),
            "error[front.first]: first\n --> <unknown>\n\nerror[front.second]: second\n --> <unknown>\n  = note: detail"
        );
    }
}
