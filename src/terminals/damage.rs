use super::types::{TerminalDamage, TerminalSurface};

/// Computes the smallest useful repaint description between two terminal surfaces.
///
/// The function chooses `Full` repaint whenever there is no previous surface or when the terminal
/// dimensions changed, because row-level diffs would no longer line up. Otherwise it compares each
/// row's cell slice and records only the rows whose contents changed. Cursor motion is folded in as
/// extra dirty rows so blinking/moved cursors repaint correctly even when the underlying text stayed
/// the same.
pub(crate) fn compute_terminal_damage(
    previous_surface: Option<&TerminalSurface>,
    surface: &TerminalSurface,
) -> TerminalDamage {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let Some(previous_surface) = previous_surface else {
        return TerminalDamage::Full;
    };
    if previous_surface.cols != surface.cols
        || previous_surface.rows != surface.rows
        || previous_surface.display_offset != surface.display_offset
    {
        return TerminalDamage::Full;
    }

    let mut dirty_rows = Vec::new();
    for y in 0..surface.rows {
        let start = y * surface.cols;
        let end = start + surface.cols;
        if previous_surface.cells[start..end] != surface.cells[start..end] {
            dirty_rows.push(y);
        }
    }

    if previous_surface.cursor != surface.cursor {
        if let Some(cursor) = previous_surface.cursor.as_ref() {
            if cursor.visible && cursor.y < surface.rows && !dirty_rows.contains(&cursor.y) {
                dirty_rows.push(cursor.y);
            }
        }
        if let Some(cursor) = surface.cursor.as_ref() {
            if cursor.visible && cursor.y < surface.rows && !dirty_rows.contains(&cursor.y) {
                dirty_rows.push(cursor.y);
            }
        }
    }

    if dirty_rows.len() >= surface.rows {
        TerminalDamage::Full
    } else {
        dirty_rows.sort_unstable();
        TerminalDamage::Rows(dirty_rows)
    }
}

#[cfg(test)]
mod tests;
