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

    pub(crate) fn reconcile_surface(&mut self, surface: &TerminalSurface) -> bool {
        let Some(selection) = self.selection.as_ref().cloned() else {
            return false;
        };
        if extract_terminal_selection_text(surface, selection.anchor, selection.focus).as_deref()
            == Some(selection.text.as_str())
        {
            return false;
        }

        if let Some((anchor, focus)) = find_shifted_terminal_selection_range(surface, &selection) {
            if anchor == selection.anchor && focus == selection.focus {
                return false;
            }
            self.selection = Some(TerminalTextSelection {
                terminal_id: selection.terminal_id,
                anchor,
                focus,
                text: selection.text,
            });
            self.presentation_revision = self.presentation_revision.wrapping_add(1);
            return true;
        }

        self.clear_selection();
        true
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PrimarySelectionSource {
    Terminal(TerminalId),
    AgentList,
}

#[derive(Resource, Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct PrimarySelectionState {
    source: Option<PrimarySelectionSource>,
    text: Option<String>,
    revision: u64,
}

impl PrimarySelectionState {
    #[cfg(test)]
    pub(crate) fn source(&self) -> Option<PrimarySelectionSource> {
        self.source
    }

    pub(crate) fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    #[cfg(test)]
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn set_terminal_selection(&mut self, terminal_id: TerminalId, text: &str) -> bool {
        self.set_selection(PrimarySelectionSource::Terminal(terminal_id), text)
    }

    pub(crate) fn set_agent_list_selection(&mut self, text: &str) -> bool {
        self.set_selection(PrimarySelectionSource::AgentList, text)
    }

    pub(crate) fn clear(&mut self) -> bool {
        if self.source.is_none() && self.text.is_none() {
            return false;
        }
        self.source = None;
        self.text = None;
        self.revision = self.revision.wrapping_add(1);
        true
    }

    fn set_selection(&mut self, source: PrimarySelectionSource, text: &str) -> bool {
        let trimmed = text.trim_end();
        if trimmed.is_empty() {
            return self.clear();
        }
        if self.source == Some(source) && self.text.as_deref() == Some(trimmed) {
            return false;
        }
        self.source = Some(source);
        self.text = Some(trimmed.to_owned());
        self.revision = self.revision.wrapping_add(1);
        true
    }
}

#[derive(Resource, Default)]
pub(crate) struct PrimarySelectionOwnerState {
    #[cfg(target_os = "linux")]
    pub(crate) child: Option<std::process::Child>,
}

fn shift_selection_point(
    point: TerminalSelectionPoint,
    row_delta: isize,
    row_count: usize,
) -> Option<TerminalSelectionPoint> {
    let row = point.row as isize + row_delta;
    if !(0..row_count as isize).contains(&row) {
        return None;
    }
    Some(TerminalSelectionPoint {
        col: point.col,
        row: row as usize,
    })
}

fn find_shifted_terminal_selection_range(
    surface: &TerminalSurface,
    selection: &TerminalTextSelection,
) -> Option<(TerminalSelectionPoint, TerminalSelectionPoint)> {
    let max_delta = surface.rows as isize;
    for magnitude in 1..=max_delta {
        for row_delta in [magnitude, -magnitude] {
            let Some(anchor) = shift_selection_point(selection.anchor, row_delta, surface.rows)
            else {
                continue;
            };
            let Some(focus) = shift_selection_point(selection.focus, row_delta, surface.rows)
            else {
                continue;
            };
            if extract_terminal_selection_text(surface, anchor, focus).as_deref()
                == Some(selection.text.as_str())
            {
                return Some((anchor, focus));
            }
        }
    }
    None
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
    use super::{
        extract_terminal_selection_text, PrimarySelectionSource, PrimarySelectionState,
        TerminalSelectionPoint, TerminalTextSelectionState,
    };
    use crate::terminals::{TerminalId, TerminalSurface};

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

    #[test]
    fn primary_selection_prefers_exact_source_and_text_identity() {
        let mut selection = PrimarySelectionState::default();

        assert!(selection.set_terminal_selection(TerminalId(7), "ABC   "));
        assert_eq!(
            selection.source(),
            Some(PrimarySelectionSource::Terminal(TerminalId(7)))
        );
        assert_eq!(selection.text(), Some("ABC"));
        let revision = selection.revision();

        assert!(!selection.set_terminal_selection(TerminalId(7), "ABC"));
        assert_eq!(selection.revision(), revision);
    }

    #[test]
    fn primary_selection_switches_owner_and_clears_cleanly() {
        let mut selection = PrimarySelectionState::default();

        assert!(selection.set_terminal_selection(TerminalId(3), "term"));
        let terminal_revision = selection.revision();
        assert!(selection.set_agent_list_selection("row text"));
        assert_eq!(selection.source(), Some(PrimarySelectionSource::AgentList));
        assert_eq!(selection.text(), Some("row text"));
        assert!(selection.revision() > terminal_revision);

        let list_revision = selection.revision();
        assert!(selection.clear());
        assert!(selection.revision() > list_revision);
        assert_eq!(selection.source(), None);
        assert_eq!(selection.text(), None);
        assert!(!selection.clear());
    }

    #[test]
    fn terminal_selection_reconcile_surface_tracks_vertical_surface_shift() {
        let mut state = TerminalTextSelectionState::default();
        state.set_selection(
            TerminalId(9),
            TerminalSelectionPoint { col: 0, row: 0 },
            TerminalSelectionPoint { col: 2, row: 0 },
            "ABC".into(),
        );

        let mut shifted = TerminalSurface::new(4, 2);
        shifted.set_text_cell(0, 1, "A");
        shifted.set_text_cell(1, 1, "B");
        shifted.set_text_cell(2, 1, "C");

        assert!(state.reconcile_surface(&shifted));
        let selection = state.selection().expect("selection should remain present");
        assert_eq!(selection.anchor, TerminalSelectionPoint { col: 0, row: 1 });
        assert_eq!(selection.focus, TerminalSelectionPoint { col: 2, row: 1 });
        assert_eq!(selection.text, "ABC");
    }

    #[test]
    fn terminal_selection_reconcile_surface_clears_when_text_disappears() {
        let mut state = TerminalTextSelectionState::default();
        state.set_selection(
            TerminalId(9),
            TerminalSelectionPoint { col: 0, row: 0 },
            TerminalSelectionPoint { col: 2, row: 0 },
            "ABC".into(),
        );

        let mut shifted = TerminalSurface::new(4, 2);
        shifted.set_text_cell(0, 1, "X");
        shifted.set_text_cell(1, 1, "Y");
        shifted.set_text_cell(2, 1, "Z");

        assert!(state.reconcile_surface(&shifted));
        assert!(state.selection().is_none());
    }
}
