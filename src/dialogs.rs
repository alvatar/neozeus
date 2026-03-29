/// Shared trait for modal dialogs that expose a fixed cyclic tab order.
///
/// Each dialog provides its ordered focus targets once, and the generic helper handles forward/back
/// traversal consistently across the app.
pub(crate) trait DialogTabOrder: Copy + Eq + 'static {
    const TAB_ORDER: &'static [Self];
}

/// Cycles one dialog focus value through its declared tab order.
///
/// Forward traversal wraps from the last target back to the first; reverse traversal wraps from the
/// first target back to the last.
pub(crate) fn cycle_dialog_focus<T: DialogTabOrder>(focus: &mut T, reverse: bool) {
    let order = T::TAB_ORDER;
    let Some(index) = order.iter().position(|candidate| candidate == focus) else {
        if let Some(first) = order.first().copied() {
            *focus = first;
        }
        return;
    };
    let next_index = if reverse {
        (index + order.len() - 1) % order.len()
    } else {
        (index + 1) % order.len()
    };
    *focus = order[next_index];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum TestFocus {
        First,
        Second,
        Third,
    }

    impl DialogTabOrder for TestFocus {
        const TAB_ORDER: &'static [Self] = &[Self::First, Self::Second, Self::Third];
    }

    /// Verifies that shared dialog focus traversal wraps in both directions.
    #[test]
    fn cycle_dialog_focus_wraps_forward_and_backward() {
        let mut focus = TestFocus::First;
        cycle_dialog_focus(&mut focus, false);
        assert_eq!(focus, TestFocus::Second);
        cycle_dialog_focus(&mut focus, false);
        assert_eq!(focus, TestFocus::Third);
        cycle_dialog_focus(&mut focus, false);
        assert_eq!(focus, TestFocus::First);
        cycle_dialog_focus(&mut focus, true);
        assert_eq!(focus, TestFocus::Third);
    }
}
