use super::*;

/// Resolves the font stack for an explicit family name.
pub(crate) fn resolve_terminal_font_report_for_family(
    requested_family: &str,
) -> Result<TerminalFontReport, String> {
    resolve_terminal_font_stack_for_family(requested_family)
}

/// Resolves the font stack for an explicit font file path.
pub(crate) fn resolve_terminal_font_report_for_path(
    path: &Path,
) -> Result<TerminalFontReport, String> {
    resolve_terminal_font_stack_for_path(path)
}
