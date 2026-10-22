//! Editor-only Dear ImGui composition. File actions, drawing and input policy
//! live in separate modules; none are shared with the game client's UI.
mod files;
mod input;
mod models;
mod windows;

#[cfg(test)]
mod tests;

use bevy::{
    app::{RunFixedMainLoop, RunFixedMainLoopSystems},
    prelude::*,
};
use dear_imgui_bevy::prelude::*;

use crate::{
    application::{EditorNotice, EditorSystems},
    presentation::props::{SelectedProp, SelectionOutlineStyle, TransformSnap},
};

pub(super) use input::{pointer_captured, viewport_input_allowed};

#[derive(Resource, Default)]
struct EditorUiState {
    settings_open: bool,
    model_node: Option<usize>,
    model_path: String,
}

pub(super) struct EditorUiPlugin;

impl Plugin for EditorUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ImguiPlugin::default())
            .init_resource::<EditorUiState>()
            // The transform window can render before the prop feature is
            // composed in a headless host; initialize its shared state here.
            .init_resource::<SelectedProp>()
            .init_resource::<TransformSnap>()
            .init_resource::<SelectionOutlineStyle>()
            .init_resource::<files::MapFilePicker>()
            .init_resource::<files::MapOpenRequest>()
            .init_resource::<files::MapOpenConfirmation>()
            .init_resource::<models::ModelBrowser>()
            .configure_sets(Update, EditorSystems::Edit.run_if(viewport_input_allowed))
            .configure_sets(
                Update,
                (EditorSystems::History, EditorSystems::Save).run_if(input::shortcuts_allowed),
            )
            .add_systems(
                RunFixedMainLoop,
                input::sync_camera_input.before(RunFixedMainLoopSystems::BeforeFixedMainLoop),
            )
            .add_systems(
                Update,
                (
                    files::open_file_shortcut.run_if(input::shortcuts_allowed),
                    files::reload_file_shortcut.run_if(input::shortcuts_allowed),
                    files::poll_map_file_picker,
                    files::open_requested_map.after(files::poll_map_file_picker),
                    models::poll_model_scan,
                    update_notice.in_set(EditorSystems::Present),
                ),
            );
        let pass = app.imgui_primary_pass().expect("editor ImGui primary pass");
        app.add_imgui_systems(&pass, pass.system(windows::draw))
            .expect("register editor ImGui UI");
    }
}

fn update_notice(time: Res<Time>, mut notice: ResMut<EditorNotice>) {
    let Some(timer) = notice.expires.as_mut() else {
        return;
    };
    timer.tick(time.delta());
    if timer.is_finished() {
        notice.message.clear();
        notice.expires = None;
    }
}
