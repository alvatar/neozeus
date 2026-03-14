use super::*;
use crate::*;

pub(crate) fn configure_terminal_fonts(
    mut font_assets: ResMut<Assets<Font>>,
    mut font_state: ResMut<TerminalFontState>,
    mut text_renderer: ResMut<TerminalTextRenderer>,
) {
    if font_state.report.is_some() {
        return;
    }

    match resolve_terminal_font_report() {
        Ok(report) => {
            match initialize_terminal_text_renderer(&report, &mut text_renderer) {
                Ok(()) => {}
                Err(error) => {
                    font_state.report = Some(Err(error));
                    return;
                }
            }

            if let Ok(primary) = load_font_handle(&mut font_assets, &report.primary.path) {
                font_state.primary_font = Some(primary);
            }

            for fallback in &report.fallbacks {
                if let Ok(handle) = load_font_handle(&mut font_assets, &fallback.path) {
                    if fallback.source.contains("private-use") {
                        font_state.private_use_font = Some(handle.clone());
                    }
                    if fallback.source.contains("emoji") {
                        font_state.emoji_font = Some(handle.clone());
                    }
                }
            }

            font_state.report = Some(Ok(report));
        }
        Err(error) => {
            font_state.report = Some(Err(error));
        }
    }
}

fn load_font_handle(font_assets: &mut Assets<Font>, path: &Path) -> Result<Handle<Font>, String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("failed to read font {}: {error}", path.display()))?;
    let font = Font::try_from_bytes(bytes)
        .map_err(|error| format!("failed to parse font {}: {error}", path.display()))?;
    Ok(font_assets.add(font))
}

pub(crate) fn initialize_terminal_text_renderer(
    report: &TerminalFontReport,
    text_renderer: &mut TerminalTextRenderer,
) -> Result<(), String> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    db.set_monospace_family(report.primary.family.clone());
    db.load_font_file(&report.primary.path).map_err(|error| {
        format!(
            "failed to load primary terminal font {} into text renderer: {error}",
            report.primary.path.display()
        )
    })?;

    for fallback in &report.fallbacks {
        db.load_font_file(&fallback.path).map_err(|error| {
            format!(
                "failed to load fallback terminal font {} into text renderer: {error}",
                fallback.path.display()
            )
        })?;
    }

    let locale = env::var("LANG").unwrap_or_else(|_| "en-US".to_owned());
    text_renderer.font_system = Some(CtFontSystem::new_with_locale_and_db(locale, db));
    text_renderer.swash_cache = CtSwashCache::new();
    Ok(())
}

pub(crate) fn resolve_terminal_font_report() -> Result<TerminalFontReport, String> {
    let requested_family = load_kitty_font_family()?.unwrap_or_else(|| "monospace".to_owned());
    let primary = fc_match_face(&requested_family, "kitty primary font")?;
    let mut fallbacks = Vec::new();
    let mut seen_paths = BTreeSet::from([primary.path.clone()]);

    for (query, source) in [
        (
            format!("{requested_family}:charset=F013"),
            "kitty fallback for private-use symbols",
        ),
        (
            format!("{requested_family}:charset=1F680"),
            "kitty fallback for emoji",
        ),
    ] {
        let candidate = fc_match_face(&query, source)?;
        if seen_paths.insert(candidate.path.clone()) {
            fallbacks.push(candidate);
        }
    }

    Ok(TerminalFontReport {
        requested_family,
        primary,
        fallbacks,
    })
}

#[derive(Default)]
pub(crate) struct KittyFontConfig {
    pub(crate) font_family: Option<String>,
}

fn load_kitty_font_family() -> Result<Option<String>, String> {
    let Some(config_path) = find_kitty_config_path() else {
        return Ok(None);
    };

    let mut visited = BTreeSet::new();
    let mut config = KittyFontConfig::default();
    parse_kitty_config_file(&config_path, &mut visited, &mut config)?;
    Ok(config.font_family)
}

pub(crate) fn find_kitty_config_path() -> Option<PathBuf> {
    if let Some(dir) = env::var_os("KITTY_CONFIG_DIRECTORY") {
        let path = PathBuf::from(dir).join("kitty.conf");
        if path.is_file() {
            return Some(path);
        }
    }

    if let Some(xdg_config_home) = env::var_os("XDG_CONFIG_HOME") {
        let path = PathBuf::from(xdg_config_home).join("kitty/kitty.conf");
        if path.is_file() {
            return Some(path);
        }
    }

    if let Some(home) = env::var_os("HOME") {
        let path = PathBuf::from(home).join(".config/kitty/kitty.conf");
        if path.is_file() {
            return Some(path);
        }
    }

    if let Some(xdg_config_dirs) = env::var_os("XDG_CONFIG_DIRS") {
        for base in env::split_paths(&xdg_config_dirs) {
            let path = base.join("kitty/kitty.conf");
            if path.is_file() {
                return Some(path);
            }
        }
    }

    let system_path = PathBuf::from("/etc/xdg/kitty/kitty.conf");
    if system_path.is_file() {
        Some(system_path)
    } else {
        None
    }
}

pub(crate) fn parse_kitty_config_file(
    path: &Path,
    visited: &mut BTreeSet<PathBuf>,
    config: &mut KittyFontConfig,
) -> Result<(), String> {
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("failed to canonicalize {}: {error}", path.display()))?;
    if !visited.insert(canonical.clone()) {
        return Ok(());
    }

    let content = fs::read_to_string(&canonical).map_err(|error| {
        format!(
            "failed to read kitty config {}: {error}",
            canonical.display()
        )
    })?;

    for line in content.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else {
            continue;
        };
        let value = parts.collect::<Vec<_>>().join(" ");
        if value.is_empty() {
            continue;
        }

        match key {
            "include" => {
                let include = canonical
                    .parent()
                    .map(|parent| parent.join(&value))
                    .unwrap_or_else(|| PathBuf::from(&value));
                if include.is_file() {
                    parse_kitty_config_file(&include, visited, config)?;
                }
            }
            "font_family" => {
                config.font_family = Some(value);
            }
            _ => {}
        }
    }

    Ok(())
}

fn fc_match_face(query: &str, source: &str) -> Result<TerminalFontFace, String> {
    let output = Command::new("/usr/bin/fc-match")
        .arg("-f")
        .arg("%{family}\n%{file}\n")
        .arg(query)
        .output()
        .map_err(|error| format!("failed to execute fc-match for `{query}`: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "fc-match failed for `{query}` with status {}: {}",
            output.status,
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let family = lines
        .next()
        .ok_or_else(|| format!("fc-match returned no family for `{query}`"))?
        .to_owned();
    let path = PathBuf::from(
        lines
            .next()
            .ok_or_else(|| format!("fc-match returned no path for `{query}`"))?,
    );

    if !path.is_file() {
        return Err(format!(
            "fc-match resolved `{query}` to missing file {}",
            path.display()
        ));
    }

    Ok(TerminalFontFace {
        family,
        path,
        source: source.to_owned(),
    })
}

pub(crate) fn sync_terminal_font_helpers(
    font_state: Res<TerminalFontState>,
    terminal_manager: Res<TerminalManager>,
    mut helper_fonts: Query<(&TerminalFontRole, &mut TextFont)>,
) {
    if !font_state.is_changed() || terminal_manager.helper_entities.is_none() {
        return;
    }

    let font_size = DEFAULT_CELL_HEIGHT_PX as f32 * 0.9;
    for (role, mut text_font) in &mut helper_fonts {
        text_font.font_size = font_size;
        match role {
            TerminalFontRole::Primary => {
                if let Some(handle) = &font_state.primary_font {
                    text_font.font = handle.clone();
                }
            }
            TerminalFontRole::PrivateUse => {
                if let Some(handle) = font_state
                    .private_use_font
                    .as_ref()
                    .or(font_state.primary_font.as_ref())
                {
                    text_font.font = handle.clone();
                }
            }
            TerminalFontRole::Emoji => {
                if let Some(handle) = font_state
                    .emoji_font
                    .as_ref()
                    .or(font_state.primary_font.as_ref())
                {
                    text_font.font = handle.clone();
                }
            }
        }
    }
}

pub(crate) fn is_private_use_like(ch: char) -> bool {
    matches!(ch as u32, 0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD)
}

pub(crate) fn is_emoji_like(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0xFE0F | 0x200D
    )
}
