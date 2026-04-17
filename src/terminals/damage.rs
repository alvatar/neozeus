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
mod tests {
    use super::*;
    use crate::tests::surface_with_text;

    /// Verifies that row-level damage includes exactly the rows whose text changed.
    ///
    /// The fixture mutates text on two different rows and expects a `Rows` damage payload listing those
    /// rows in sorted order instead of promoting the whole surface to a full repaint.
    #[test]
    fn compute_terminal_damage_marks_only_changed_rows() {
        let previous = surface_with_text(3, 4, 1, "ab");
        let next = surface_with_text(3, 4, 2, "cd");
        assert_eq!(
            compute_terminal_damage(Some(&previous), &next),
            TerminalDamage::Rows(vec![1, 2])
        );
    }

    /// Verifies that any surface resize forces a full repaint.
    ///
    /// Row diffs are only meaningful when the old and new grids share the same geometry. This test locks
    /// down the rule that changing the column count promotes the result to `TerminalDamage::Full`.
    #[test]
    fn compute_terminal_damage_marks_resize_as_full() {
        let previous = TerminalSurface::new(4, 3);
        let next = TerminalSurface::new(5, 3);
        assert_eq!(
            compute_terminal_damage(Some(&previous), &next),
            TerminalDamage::Full
        );
    }

    #[test]
    fn compute_terminal_damage_marks_viewport_scroll_as_full() {
        let mut previous = surface_with_text(3, 4, 1, "ab");
        let mut next = previous.clone();
        previous.display_offset = 0;
        next.display_offset = 1;
        assert_eq!(
            compute_terminal_damage(Some(&previous), &next),
            TerminalDamage::Full
        );
    }
}
