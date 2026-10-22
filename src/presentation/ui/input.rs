//! Shared input-capture policy for editor tools and the free camera.
use bevy::{camera_controller::free_camera::FreeCameraState, prelude::*};
use dear_imgui_bevy::prelude::*;

use crate::application::control_pressed;

pub(super) fn shortcuts_allowed(capture: Option<Res<ImguiInputCapture>>) -> bool {
    capture.is_none_or(|capture| capture_allows_shortcuts(capture.primary()))
}

fn capture_allows_shortcuts(capture: ImguiInputCaptureState) -> bool {
    !capture.wants_keyboard_input() && !capture.wants_text_input()
}

/// Optional so independent, device-free editor systems/tests need no UI host.
pub(in crate::presentation) fn viewport_input_allowed(
    capture: Option<Res<ImguiInputCapture>>,
) -> bool {
    capture.is_none_or(|capture| capture_allows_viewport(capture.primary()))
}

fn capture_allows_viewport(capture: ImguiInputCaptureState) -> bool {
    !capture.wants_any_input()
}

pub(in crate::presentation) fn pointer_captured(capture: Option<&ImguiInputCapture>) -> bool {
    capture.is_some_and(|capture| capture.primary().wants_pointer_input())
}

pub(super) fn sync_camera_input(
    capture: Option<Res<ImguiInputCapture>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut cameras: Query<&mut FreeCameraState>,
) {
    let input_allowed = viewport_input_allowed(capture) && !control_pressed(&keys);
    for mut camera in &mut cameras {
        camera.enabled = input_allowed;
        if !input_allowed {
            camera.velocity = Vec3::ZERO;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_capture_does_not_block_keyboard_shortcuts() {
        assert!(shortcuts_allowed(None));
        assert!(capture_allows_shortcuts(ImguiInputCaptureState {
            want_capture_mouse: true,
            ..default()
        }));
        assert!(!capture_allows_shortcuts(ImguiInputCaptureState {
            want_capture_keyboard: true,
            ..default()
        }));
        assert!(!capture_allows_shortcuts(ImguiInputCaptureState {
            want_text_input: true,
            ..default()
        }));
    }

    #[test]
    fn either_control_key_stops_camera_and_clears_velocity() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, sync_camera_input);
        let camera = app.world_mut().spawn(FreeCameraState::default()).id();
        for control in [KeyCode::ControlLeft, KeyCode::ControlRight] {
            app.world_mut()
                .get_mut::<FreeCameraState>(camera)
                .unwrap()
                .velocity = Vec3::X;
            {
                let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
                keys.press(control);
                keys.press(KeyCode::KeyW);
            }
            app.update();
            let state = app.world().get::<FreeCameraState>(camera).unwrap();
            assert!(!state.enabled);
            assert_eq!(state.velocity, Vec3::ZERO);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .release(control);
            app.update();
            assert!(app.world().get::<FreeCameraState>(camera).unwrap().enabled);
        }
    }

    #[test]
    fn ui_capture_blocks_viewport_input() {
        assert!(viewport_input_allowed(None));
        assert!(capture_allows_viewport(ImguiInputCaptureState::default()));
        for capture in [
            ImguiInputCaptureState {
                want_capture_mouse: true,
                ..default()
            },
            ImguiInputCaptureState {
                want_capture_keyboard: true,
                ..default()
            },
            ImguiInputCaptureState {
                want_text_input: true,
                ..default()
            },
        ] {
            assert!(!capture_allows_viewport(capture));
        }
    }
}
