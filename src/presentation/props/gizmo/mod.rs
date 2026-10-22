mod view;

use bevy::prelude::*;

use super::{SelectedProp, TransformSnap};
use crate::{
    application::{EditorDocument, EditorSystems, control_pressed},
    infrastructure::content::{LifMapNodeIndex, LifPropRef},
    presentation::viewport::EditorCamera,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum GizmoMode {
    #[default]
    Translate,
    Rotate,
}

#[derive(Resource, Default)]
struct PropGizmo {
    mode: GizmoMode,
    drag: Option<GizmoDrag>,
}

#[derive(Clone, Copy)]
struct GizmoDrag {
    axis: Entity,
    prop: Entity,
    initial: Transform,
    amount: f32,
}

#[derive(Component, Clone, Copy)]
struct GizmoAxis {
    axis: Vec3,
}

pub(super) struct PropGizmoPlugin;

impl Plugin for PropGizmoPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PropGizmo>()
            .add_systems(Startup, view::setup)
            .add_systems(Update, switch_gizmo_mode.in_set(EditorSystems::Edit))
            .add_systems(
                Update,
                (view::sync_prop_gizmo, view::draw_prop_gizmo).in_set(EditorSystems::Present),
            )
            .add_observer(gizmo_drag_start)
            .add_observer(gizmo_drag)
            .add_observer(gizmo_drag_end);
    }
}

fn switch_gizmo_mode(keys: Res<ButtonInput<KeyCode>>, mut gizmo: ResMut<PropGizmo>) {
    // Ctrl+R is reserved for reloading the document, not rotate mode.
    if control_pressed(&keys) {
        return;
    }
    if keys.just_pressed(KeyCode::KeyG) {
        gizmo.mode = GizmoMode::Translate;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        gizmo.mode = GizmoMode::Rotate;
    }
}

fn gizmo_drag_start(
    drag: On<Pointer<DragStart>>,
    axes: Query<&GizmoAxis>,
    props: Query<&Transform, With<LifPropRef>>,
    selected: Res<SelectedProp>,
    mut state: ResMut<PropGizmo>,
    capture: Option<Res<dear_imgui_bevy::prelude::ImguiInputCapture>>,
) {
    if crate::presentation::ui::pointer_captured(capture.as_deref()) {
        return;
    }
    if drag.button != PointerButton::Primary {
        return;
    }
    let Ok(_) = axes.get(drag.entity) else { return };
    let Some(prop) = selected.0 else { return };
    let Ok(initial) = props.get(prop) else {
        return;
    };
    state.drag = Some(GizmoDrag {
        axis: drag.entity,
        prop,
        initial: *initial,
        amount: 0.0,
    });
}

fn gizmo_drag(
    drag: On<Pointer<Drag>>,
    axes: Query<&GizmoAxis>,
    cameras: Query<&GlobalTransform, With<EditorCamera>>,
    mut props: Query<(&mut Transform, &LifMapNodeIndex), With<LifPropRef>>,
    document: Option<ResMut<EditorDocument>>,
    mut state: ResMut<PropGizmo>,
    snap: Res<TransformSnap>,
    capture: Option<Res<dear_imgui_bevy::prelude::ImguiInputCapture>>,
) {
    if crate::presentation::ui::pointer_captured(capture.as_deref()) {
        return;
    }
    let Some(mut document) = document else {
        return;
    };
    if drag.button != PointerButton::Primary {
        return;
    }
    let mode = state.mode;
    let Some(drag_state) = state.drag.as_mut() else {
        return;
    };
    if drag_state.axis != drag.entity {
        return;
    }
    let Ok(axis) = axes.get(drag.entity) else {
        return;
    };
    let Ok(camera) = cameras.single() else { return };
    let Ok((mut transform, node)) = props.get_mut(drag_state.prop) else {
        return;
    };
    let axis = axis.axis;
    let right = camera.right().as_vec3();
    let up = camera.up().as_vec3();
    let screen_axis = Vec2::new(axis.dot(right), -axis.dot(up));
    let amount = if screen_axis.length_squared() > 0.001 {
        drag.delta.dot(screen_axis.normalize()) * 0.08
    } else {
        0.0
    };
    drag_state.amount += amount;
    if mode == GizmoMode::Translate {
        transform.translation = drag_state.initial.translation
            + axis * snapped_translation_amount(drag_state.amount, &snap);
    } else {
        transform.rotation =
            Quat::from_axis_angle(axis, snapped_rotation_amount(drag_state.amount, &snap))
                * drag_state.initial.rotation;
    }
    if let Err(error) = document.0.update_prop_transform(
        node.0,
        transform.translation.to_array(),
        transform.rotation.to_array(),
        transform.scale.to_array(),
    ) {
        error!("recording gizmo transform: {error:#}");
    }
}

fn gizmo_drag_end(
    drag: On<Pointer<DragEnd>>,
    mut state: ResMut<PropGizmo>,
    document: Option<ResMut<EditorDocument>>,
) {
    let Some(mut document) = document else {
        return;
    };
    if drag.button != PointerButton::Primary {
        return;
    }
    let Some(drag_state) = state.drag else {
        return;
    };
    if drag_state.axis != drag.entity {
        return;
    }
    state.drag = None;
    document.0.commit_pending_prop_transform();
}

fn snapped_translation_amount(amount: f32, snap: &TransformSnap) -> f32 {
    if snap.enabled && snap.translation_step > f32::EPSILON {
        (amount / snap.translation_step).round() * snap.translation_step
    } else {
        amount
    }
}

fn snapped_rotation_amount(amount: f32, snap: &TransformSnap) -> f32 {
    let radians = amount * 0.025;
    let step = snap.rotation_degrees.to_radians();
    if snap.enabled && step > f32::EPSILON {
        (radians / step).round() * step
    } else {
        radians
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;
    use bevy::{
        camera::NormalizedRenderTarget,
        picking::pointer::{Location, PointerId},
        window::WindowRef,
    };

    #[test]
    fn release_coalesces_history_without_saving_and_ignores_right_button() {
        let (_directory, mut document) = test_support::document();
        let path = document.source_path.clone();
        let bytes = std::fs::read(&path).unwrap();
        for x in [1.0, 2.0] {
            document
                .update_prop_transform(0, [x, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0], [1.0; 3])
                .unwrap();
        }
        let mut app = App::new();
        let axis = app.world_mut().spawn_empty().id();
        let prop = app.world_mut().spawn_empty().id();
        let window = app.world_mut().spawn_empty().id();
        app.insert_resource(EditorDocument(document))
            .insert_resource(PropGizmo {
                mode: GizmoMode::Translate,
                drag: Some(GizmoDrag {
                    axis,
                    prop,
                    initial: Transform::default(),
                    amount: 0.0,
                }),
            })
            .add_observer(gizmo_drag_end);
        for button in [PointerButton::Secondary, PointerButton::Primary] {
            app.world_mut().trigger(Pointer::new(
                PointerId::Mouse,
                Location {
                    target: NormalizedRenderTarget::Window(
                        WindowRef::Primary.normalize(Some(window)).unwrap(),
                    ),
                    position: Vec2::ZERO,
                },
                DragEnd {
                    button,
                    distance: Vec2::ONE,
                },
                axis,
            ));
            if button == PointerButton::Secondary {
                assert!(app.world().resource::<PropGizmo>().drag.is_some());
                assert!(
                    app.world()
                        .resource::<EditorDocument>()
                        .0
                        .has_pending_prop_transform()
                );
            }
        }
        assert!(app.world().resource::<PropGizmo>().drag.is_none());
        let mut document = app.world_mut().resource_mut::<EditorDocument>();
        assert!(!document.0.has_pending_prop_transform());
        assert!(document.0.undo());
        assert_eq!(document.0.prop_transform(0).unwrap().0, [0.0; 3]);
        assert!(!document.0.undo(), "one undo entry per drag");
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn control_r_does_not_switch_to_rotate_mode() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<PropGizmo>()
            .add_systems(Update, switch_gizmo_mode);
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::KeyR);

        app.update();

        assert_eq!(
            app.world().resource::<PropGizmo>().mode,
            GizmoMode::Translate
        );
    }

    #[test]
    fn snapping_rounds_gizmo_drag_amounts_to_configured_increments() {
        let snap = TransformSnap {
            enabled: true,
            translation_step: 5.0,
            rotation_degrees: 15.0,
        };
        assert_eq!(snapped_translation_amount(2.6, &snap), 5.0);
        assert!((snapped_rotation_amount(12.0, &snap) - 15.0f32.to_radians()).abs() < 0.0001);
    }
}
