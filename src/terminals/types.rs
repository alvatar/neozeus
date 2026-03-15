use alacritty_terminal::grid::Dimensions;
use bevy_egui::egui;
use portable_pty::{Child, MasterPty};
use std::{io::Write, path::PathBuf, sync::Arc, time::Duration};

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum TerminalCellContent {
    #[default]
    Empty,
    Single(char),
    InlineSmall([char; 2], u8),
    Heap(Arc<str>),
}

impl TerminalCellContent {
    pub(crate) fn from_parts(base: char, extra: Option<&[char]>) -> Self {
        let Some(extra) = extra else {
            return Self::Single(base);
        };
        match extra {
            [] => Self::Single(base),
            [first] => Self::InlineSmall([base, *first], 2),
            _ => {
                let mut text = String::with_capacity(1 + extra.len());
                text.push(base);
                for character in extra {
                    text.push(*character);
                }
                Self::Heap(Arc::<str>::from(text))
            }
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    pub(crate) fn any_char(&self, mut predicate: impl FnMut(char) -> bool) -> bool {
        match self {
            Self::Empty => false,
            Self::Single(ch) => predicate(*ch),
            Self::InlineSmall(chars, len) => chars[..usize::from(*len)]
                .iter()
                .copied()
                .any(&mut predicate),
            Self::Heap(text) => text.chars().any(predicate),
        }
    }

    pub(crate) fn to_owned_string(&self) -> String {
        match self {
            Self::Empty => String::new(),
            Self::Single(ch) => ch.to_string(),
            Self::InlineSmall(chars, len) => chars[..usize::from(*len)].iter().collect(),
            Self::Heap(text) => text.as_ref().to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TerminalCell {
    pub(crate) content: TerminalCellContent,
    pub(crate) fg: egui::Color32,
    pub(crate) bg: egui::Color32,
    pub(crate) width: u8,
}

impl Default for TerminalCell {
    fn default() -> Self {
        Self {
            content: TerminalCellContent::Empty,
            fg: egui::Color32::from_rgb(220, 220, 220),
            bg: crate::app_config::DEFAULT_BG,
            width: 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalCursorShape {
    Block,
    Underline,
    Beam,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TerminalCursor {
    pub(crate) x: usize,
    pub(crate) y: usize,
    pub(crate) shape: TerminalCursorShape,
    pub(crate) visible: bool,
    pub(crate) color: egui::Color32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TerminalSurface {
    pub(crate) cols: usize,
    pub(crate) rows: usize,
    pub(crate) cells: Vec<TerminalCell>,
    pub(crate) cursor: Option<TerminalCursor>,
}

impl TerminalSurface {
    pub(crate) fn new(cols: usize, rows: usize) -> Self {
        Self {
            cols,
            rows,
            cells: vec![TerminalCell::default(); cols.saturating_mul(rows)],
            cursor: None,
        }
    }

    pub(crate) fn set_cell(&mut self, x: usize, y: usize, cell: TerminalCell) {
        if x >= self.cols || y >= self.rows {
            return;
        }
        self.cells[y * self.cols + x] = cell;
    }

    #[cfg(test)]
    pub(crate) fn set_text_cell(&mut self, x: usize, y: usize, text: &str) {
        let mut chars = text.chars();
        let Some(base) = chars.next() else {
            self.set_cell(x, y, TerminalCell::default());
            return;
        };
        let extra = chars.collect::<Vec<_>>();
        self.set_cell(
            x,
            y,
            TerminalCell {
                content: TerminalCellContent::from_parts(base, Some(&extra)),
                ..Default::default()
            },
        );
    }

    pub(crate) fn cell(&self, x: usize, y: usize) -> &TerminalCell {
        &self.cells[y * self.cols + x]
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum TerminalDamage {
    #[default]
    Full,
    Rows(Vec<usize>),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum TerminalLifecycle {
    #[default]
    Running,
    Exited {
        code: Option<u32>,
        signal: Option<String>,
    },
    Disconnected,
    Failed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TerminalRuntimeState {
    pub(crate) status: String,
    pub(crate) lifecycle: TerminalLifecycle,
    pub(crate) last_error: Option<String>,
}

impl TerminalRuntimeState {
    pub(crate) fn running(status: impl Into<String>) -> Self {
        Self {
            status: status.into(),
            lifecycle: TerminalLifecycle::Running,
            last_error: None,
        }
    }

    pub(crate) fn failed(status: impl Into<String>) -> Self {
        let status = status.into();
        Self {
            status: status.clone(),
            lifecycle: TerminalLifecycle::Failed,
            last_error: Some(status),
        }
    }

    pub(crate) fn disconnected(status: impl Into<String>) -> Self {
        Self {
            status: status.into(),
            lifecycle: TerminalLifecycle::Disconnected,
            last_error: None,
        }
    }

    pub(crate) fn exited(
        status: impl Into<String>,
        code: Option<u32>,
        signal: Option<String>,
    ) -> Self {
        Self {
            status: status.into(),
            lifecycle: TerminalLifecycle::Exited { code, signal },
            last_error: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TerminalSnapshot {
    pub(crate) surface: Option<TerminalSurface>,
    pub(crate) runtime: TerminalRuntimeState,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TerminalFrameUpdate {
    pub(crate) surface: TerminalSurface,
    pub(crate) damage: TerminalDamage,
    pub(crate) runtime: TerminalRuntimeState,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TerminalUpdate {
    Frame(TerminalFrameUpdate),
    Status {
        runtime: TerminalRuntimeState,
        surface: Option<TerminalSurface>,
    },
}

pub(crate) type LatestTerminalStatus = (TerminalRuntimeState, Option<TerminalSurface>);
pub(crate) type DrainedTerminalUpdates = (
    Option<TerminalFrameUpdate>,
    Option<LatestTerminalStatus>,
    u64,
);

pub(crate) enum TerminalCommand {
    InputText(String),
    InputEvent(String),
    SendCommand(String),
    ScrollDisplay(i32),
}

pub(crate) struct TerminalDimensions {
    pub(crate) cols: usize,
    pub(crate) rows: usize,
}

impl Dimensions for TerminalDimensions {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

pub(crate) struct PtySession {
    pub(crate) master: Box<dyn MasterPty + Send>,
    pub(crate) writer: Box<dyn Write + Send>,
    pub(crate) child: Box<dyn Child + Send + Sync>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalFontFace {
    pub(crate) family: String,
    pub(crate) path: PathBuf,
    pub(crate) source: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalFontReport {
    pub(crate) requested_family: String,
    pub(crate) primary: TerminalFontFace,
    pub(crate) fallbacks: Vec<TerminalFontFace>,
}

pub(crate) const PTY_OUTPUT_WAIT_TIMEOUT: Duration = Duration::from_millis(16);
pub(crate) const PTY_OUTPUT_BATCH_WINDOW: Duration = Duration::from_millis(16);
pub(crate) const PTY_OUTPUT_BATCH_BYTES: usize = 512 * 1024;
