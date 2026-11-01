//! The toolkit's conformance run against the gizmo tool.

#![cfg(feature = "ui")]

use std::sync::Arc;

use glam::Vec3;
use viewport_lib_gizmo::ui::{GIZMO_KIND, GizmoFrame, GizmoTool};
use viewport_lib_types::camera::Camera;
use viewport_lib_ui_extend::events::UiEvent;
use viewport_lib_ui_extend::serde_json;
use viewport_lib_ui_extend::sources::HostMap;
use viewport_lib_ui_test::Conformance;
use viewport_lib_ui_test::conformance::{Check, Subject};

/// A frame on a selection at the origin, so every pane has handles to draw and the
/// drive the checks run lands on the gizmo's centre.
fn framed() -> HostMap<'static> {
    let camera = Camera::orbit(glam::DVec3::ZERO, 6.0, Camera::default().orientation());
    HostMap::new().with_context(GizmoFrame::new(camera).with_selection_centre(Some(Vec3::ZERO)))
}

/// The run: the gizmo authors no params, so its nodes carry `null`. The boxes are
/// larger than the default so the handles have room.
fn conformance() -> Conformance {
    Conformance::new(
        GIZMO_KIND,
        Arc::new(GizmoTool::new()),
        serde_json::Value::Null,
    )
    .sized([320.0, 240.0])
    .with_map(framed())
}

#[test]
fn the_gizmo_passes_the_conformance_run() {
    // The standard checks read state through what a node draws after an idle frame,
    // where the gizmo draws the same whatever it kept, and its events are reports
    // rather than events about widgets. So the run also checks that driving a pane
    // reports a result naming that pane, which gives the input checks something to
    // look at.
    let reports = Check::new("driving-reports-a-result", |s: &Subject| {
        let mut h = s.harness(s.screen_of(&[Subject::FIRST]));
        s.idle(&mut h);
        let events = s.drive(&mut h, Subject::FIRST);
        let named: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                UiEvent::CustomOutput { id, .. } => Some(*id),
                _ => None,
            })
            .collect();
        if !named.is_empty() && named.iter().all(|id| *id == Subject::FIRST) {
            Ok(())
        } else {
            Err(format!(
                "driving node {} reported results from {named:?}",
                Subject::FIRST
            ))
        }
    });
    let report = conformance().with_check(reports).run();
    assert!(report.passed(), "{report}");
}
