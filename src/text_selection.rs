use crate::{
    hud::AgentListRowKey,
    terminals::{TerminalId, TerminalSurface},
};
use bevy::prelude::{Resource, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TerminalSelectionPoint {
    pub(crate) col: usize,
    pub(crate) row: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalTextSelection {
    pub(crate) terminal_id: TerminalId,
    pub(crate) anchor: TerminalSelectionPoint,
    pub(crate) focus: TerminalSelectionPoint,
    pub(crate) text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TerminalTextSelectionDrag {
    pub(crate) terminal_id: TerminalId,
    pub(crate) anchor: TerminalSelectionPoint,
}

#[derive(Resource, Default)]
pub(crate) struct TerminalTextSelectionState {
    pub(crate) drag: Option<TerminalTextSelectionDrag>,
    selection: Option<TerminalTextSelection>,
    presentation_revision: u64,
}

impl TerminalTextSelectionState {
    pub(crate) fn selection(&self) -> Option<&TerminalTextSelection> {
        self.selection.as_ref()
    }

    pub(crate) fn selection_for(&self, terminal_id: TerminalId) -> Option<&TerminalTextSelection> {
        self.selection
            .as_ref()
            .filter(|selection| selection.terminal_id == terminal_id)
    }

    pub(crate) fn presentation_revision_for(&self, terminal_id: TerminalId) -> Option<u64> {
        self.selection_for(terminal_id)
            .map(|_| self.presentation_revision)
    }

    pub(crate) fn begin_drag(&mut self, terminal_id: TerminalId, anchor: TerminalSelectionPoint) {
        self.drag = Some(TerminalTextSelectionDrag {
            terminal_id,
            anchor,
        });
    }

    pub(crate) fn clear_drag(&mut self) {
        self.drag = None;
    }

    pub(crate) fn clear_selection(&mut self) {
        if self.selection.take().is_some() {
            self.presentation_revision = self.presentation_revision.wrapping_add(1);
        }
    }

    pub(crate) fn set_selection(
        &mut self,
        terminal_id: TerminalId,
        anchor: TerminalSelectionPoint,
        focus: TerminalSelectionPoint,
        text: String,
    ) {
        let next = TerminalTextSelection {
            terminal_id,
            anchor,
            focus,
            text,
        };
        if self.selection.as_ref() == Some(&next) {
            return;
        }
        self.selection = Some(next);
        self.presentation_revision = self.presentation_revision.wrapping_add(1);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentListTextSelection {
    pub(crate) anchor_row: AgentListRowKey,
    pub(crate) focus_row: AgentListRowKey,
    pub(crate) text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AgentListTextSelectionDrag {
    pub(crate) anchor_row: AgentListRowKey,
    pub(crate) press_origin: Vec2,
}

#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub(crate) struct AgentListTextSelectionState {
    pub(crate) drag: Option<AgentListTextSelectionDrag>,
    selection: Option<AgentListTextSelection>,
}

impl AgentListTextSelectionState {
    pub(crate) fn selection(&self) -> Option<&AgentListTextSelection> {
        self.selection.as_ref()
    }

    pub(crate) fn begin_drag(&mut self, anchor_row: AgentListRowKey, press_origin: Vec2) {
        self.drag = Some(AgentListTextSelectionDrag {
            anchor_row,
            press_origin,
        });
    }

    pub(crate) fn clear_drag(&mut self) {
        self.drag = None;
    }

    pub(crate) fn clear_selection(&mut self) {
        self.selection = None;
    }

    pub(crate) fn set_selection(
        &mut self,
        anchor_row: AgentListRowKey,
        focus_row: AgentListRowKey,
        text: String,
    ) {
        let next = AgentListTextSelection {
            anchor_row,
            focus_row,
            text,
        };
        if self.selection.as_ref() == Some(&next) {
            return;
        }
        self.selection = Some(next);
    }
}

#[derive(Resource, Default)]
pub(crate) struct PrimarySelectionOwnerState {
    #[cfg(target_os = "linux")]
    pub(crate) child: Option<std::process::Child>,
}

pub(crate) fn extract_terminal_selection_text(
    surface: &TerminalSurface,
    anchor: TerminalSelectionPoint,
    focus: TerminalSelectionPoint,
) -> Option<String> {
    if surface.cols == 0 || surface.rows == 0 || anchor == focus {
        return None;
    }

    let (start, end) = if (anchor.row, anchor.col) <= (focus.row, focus.col) {
        (anchor, focus)
    } else {
        (focus, anchor)
    };

    let mut lines = Vec::new();
    for row in start.row..=end.row {
        let start_col = if row == start.row { start.col } else { 0 };
        let end_col = if row == end.row {
            end.col
        } else {
            surface.cols.saturating_sub(1)
        };
        if start_col >= surface.cols || end_col >= surface.cols || start_col > end_col {
            continue;
        }
        let mut line = String::new();
        for col in start_col..=end_col {
            let cell = surface.cell(col, row);
            if cell.width == 0 {
                continue;
            }
            if cell.content.is_empty() {
                line.push(' ');
            } else {
                line.push_str(&cell.content.to_owned_string());
            }
        }
        let trimmed = line.trim_end_matches(' ');
        lines.push(trimmed.to_owned());
    }

    let text = lines.join("\n");
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::{extract_terminal_selection_text, TerminalSelectionPoint};
    use crate::terminals::TerminalSurface;

    #[test]
    fn extract_terminal_selection_text_spans_rows_and_trims_trailing_blanks() {
        let mut surface = TerminalSurface::new(6, 2);
        surface.set_text_cell(0, 0, "A");
        surface.set_text_cell(1, 0, "B");
        surface.set_text_cell(2, 0, "C");
        surface.set_text_cell(0, 1, "D");
        surface.set_text_cell(1, 1, "E");

        let text = extract_terminal_selection_text(
            &surface,
            TerminalSelectionPoint { col: 1, row: 0 },
            TerminalSelectionPoint { col: 3, row: 1 },
        )
        .expect("selection text should exist");

        assert_eq!(text, "BC\nDE");
    }

    #[test]
    fn extract_terminal_selection_text_returns_none_for_empty_range() {
        let surface = TerminalSurface::new(4, 2);
        assert!(extract_terminal_selection_text(
            &surface,
            TerminalSelectionPoint { col: 1, row: 0 },
            TerminalSelectionPoint { col: 1, row: 0 }
        )
        .is_none());
    }
}
