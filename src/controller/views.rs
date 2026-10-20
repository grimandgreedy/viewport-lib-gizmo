//! Choosing which of several views drives the gizmo.

/// Tracks which of several views (the quadrants of a split layout, say) drives
/// the [`GizmoDriver`](super::GizmoDriver) each frame.
///
/// The driving view is the hovered one, except that a view keeps driving while
/// a drag or a gizmo session it started is under way, even after the cursor
/// moves over another view. A session's drag is measured in the starting view's
/// pixels and through its camera, so handing it to another view mid-drag would
/// make the object jump.
///
/// Each frame, read [`current`](Self::current), feed that view's camera, input
/// and view-local cursor to the driver, then call [`end_frame`](Self::end_frame).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActiveView {
    held: Option<usize>,
}

impl ActiveView {
    /// No view held.
    pub fn new() -> Self {
        Self::default()
    }

    /// The view to drive this frame: the one held from an earlier frame, or
    /// `hovered` when none is held.
    pub fn current(&self, hovered: Option<usize>) -> Option<usize> {
        self.held.or(hovered)
    }

    /// Hold `view` while `busy` (a pointer button down, or
    /// [`GizmoDriver::is_active`](super::GizmoDriver::is_active)), and release
    /// it once neither is true. A view already held stays held.
    pub fn end_frame(&mut self, view: Option<usize>, busy: bool) {
        self.held = if busy { self.held.or(view) } else { None };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_the_hovered_view_when_idle() {
        let mut v = ActiveView::new();
        assert_eq!(v.current(Some(2)), Some(2));
        v.end_frame(Some(2), false);
        assert_eq!(v.current(Some(1)), Some(1));
    }

    #[test]
    fn holds_the_starting_view_while_busy() {
        let mut v = ActiveView::new();
        let start = v.current(Some(0));
        v.end_frame(start, true);
        // The cursor crosses into view 3 mid-drag: view 0 keeps driving.
        assert_eq!(v.current(Some(3)), Some(0));
        v.end_frame(Some(0), true);
        assert_eq!(v.current(None), Some(0));
        // The drag ends: the hovered view drives again.
        v.end_frame(Some(0), false);
        assert_eq!(v.current(Some(3)), Some(3));
    }
}
