use crate::*;

#[derive(Resource, Default)]
pub(crate) struct TerminalFontState {
    pub(crate) report: Option<Result<TerminalFontReport, String>>,
    pub(crate) primary_font: Option<Handle<Font>>,
    pub(crate) private_use_font: Option<Handle<Font>>,
    pub(crate) emoji_font: Option<Handle<Font>>,
}

#[derive(Resource)]
pub(crate) struct TerminalTextRenderer {
    pub(crate) font_system: Option<CtFontSystem>,
    pub(crate) swash_cache: CtSwashCache,
}

impl Default for TerminalTextRenderer {
    fn default() -> Self {
        Self {
            font_system: None,
            swash_cache: CtSwashCache::new(),
        }
    }
}

#[derive(Resource)]
pub(crate) struct TerminalPlaneState {
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
    pub(crate) distance: f32,
    pub(crate) focal_length: f32,
    pub(crate) offset: Vec2,
}

#[derive(Resource, Default)]
pub(crate) struct TerminalPointerState {
    pub(crate) scroll_drag_remainder_px: f32,
}

impl Default for TerminalPlaneState {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.0,
            distance: 10.0,
            focal_length: 10.0,
            offset: Vec2::ZERO,
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct TerminalTextureState {
    pub(crate) image: Option<Handle<Image>>,
    pub(crate) helper_entities: Option<TerminalFontEntities>,
    pub(crate) texture_size: UVec2,
    pub(crate) cell_size: UVec2,
}

#[derive(Resource, Default)]
pub(crate) struct TerminalGlyphCache {
    pub(crate) glyphs: HashMap<TerminalGlyphCacheKey, CachedTerminalGlyph>,
}

#[derive(Clone, Copy)]
pub(crate) struct TerminalFontEntities {
    pub(crate) primary: Entity,
    pub(crate) private_use: Entity,
    pub(crate) emoji: Entity,
}

#[derive(Component)]
pub(crate) struct TerminalPlaneMarker;

#[derive(Component)]
pub(crate) struct TerminalCameraMarker;

#[derive(Component)]
pub(crate) struct TerminalHudSurfaceMarker;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum TerminalFontRole {
    Primary,
    PrivateUse,
    Emoji,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TerminalGlyphCacheKey {
    pub(crate) content: TerminalCellContent,
    pub(crate) font_role: TerminalFontRole,
    pub(crate) width_cells: u8,
    pub(crate) cell_width: u32,
    pub(crate) cell_height: u32,
}

#[derive(Clone)]
pub(crate) struct CachedTerminalGlyph {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<u8>,
    pub(crate) preserve_color: bool,
}

#[derive(Clone, Default, PartialEq)]
pub(crate) struct TerminalSnapshot {
    pub(crate) surface: Option<TerminalSurface>,
    pub(crate) status: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum TerminalDamage {
    #[default]
    Full,
    Rows(Vec<usize>),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TerminalFrameUpdate {
    pub(crate) surface: TerminalSurface,
    pub(crate) damage: TerminalDamage,
    pub(crate) status: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TerminalUpdate {
    Frame(TerminalFrameUpdate),
    Status {
        status: String,
        surface: Option<TerminalSurface>,
    },
}

pub(crate) type LatestTerminalStatus = (String, Option<TerminalSurface>);
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
    Shutdown,
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
            bg: DEFAULT_BG,
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
        let index = y * self.cols + x;
        self.cells[index] = cell;
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
