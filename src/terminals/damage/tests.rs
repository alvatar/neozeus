use super::compute_terminal_damage;
use crate::terminals::{TerminalDamage, TerminalSurface};
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
