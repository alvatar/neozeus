use super::compute_terminal_damage;
use crate::terminals::{TerminalDamage, TerminalSurface};
use crate::tests::surface_with_text;

#[test]
fn compute_terminal_damage_marks_only_changed_rows() {
    let previous = surface_with_text(3, 4, 1, "ab");
    let next = surface_with_text(3, 4, 2, "cd");
    assert_eq!(
        compute_terminal_damage(Some(&previous), &next),
        TerminalDamage::Rows(vec![1, 2])
    );
}

#[test]
fn compute_terminal_damage_marks_resize_as_full() {
    let previous = TerminalSurface::new(4, 3);
    let next = TerminalSurface::new(5, 3);
    assert_eq!(
        compute_terminal_damage(Some(&previous), &next),
        TerminalDamage::Full
    );
}
