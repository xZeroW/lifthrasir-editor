//! Immediate-mode drawing; document/file operations stay in their own modules.
use bevy::prelude::*;
use dear_imgui_bevy::prelude::ImguiFrame;
use dear_imgui_rs::{Condition, TreeNodeFlags, Ui, WindowFlags};

use super::{
    EditorUiState,
    files::{
        MapFilePicker, MapOpenConfirmation, MapOpenRequest, cancel_map_open, confirm_map_open,
        start_map_file_picker,
    },
    models::{ModelBrowser, model_scan_active, start_model_scan},
};
use crate::{
    application::{EditorDocument, EditorNotice, save},
    infrastructure::content::{LifMapNodeIndex, LifPropRef},
    presentation::props::{SelectedProp, SelectionOutlineStyle, TransformSnap},
    presentation::terrain_grid::TerrainGridStyle,
};

pub(super) fn draw(
    frame: ImguiFrame<'_>,
    mut state: ResMut<EditorUiState>,
    mut style: ResMut<TerrainGridStyle>,
    mut selection_style: ResMut<SelectionOutlineStyle>,
    mut picker: ResMut<MapFilePicker>,
    mut open_request: ResMut<MapOpenRequest>,
    mut open_confirmation: ResMut<MapOpenConfirmation>,
    mut model_browser: ResMut<ModelBrowser>,
    document: Option<ResMut<EditorDocument>>,
    selected: Res<SelectedProp>,
    props: Query<(&LifMapNodeIndex, &Transform, &LifPropRef), With<LifPropRef>>,
    mut snap: ResMut<TransformSnap>,
    mut notice: ResMut<EditorNotice>,
    mut exit: MessageWriter<AppExit>,
) {
    let ui = frame.ui();
    let mut document = document;
    ui.main_menu_bar(|| {
        ui.menu("File", || {
            if ui.menu_item_with_shortcut("Open File", "Ctrl+O") {
                start_map_file_picker(&mut picker);
            }
            if ui.menu_item_with_shortcut("Reload File", "Ctrl+R") {
                if let Some(document) = document.as_ref() {
                    super::files::request_map_open(
                        document.0.source_path.clone(),
                        Some(document),
                        &mut open_request,
                        &mut open_confirmation,
                    );
                } else {
                    notice.show("Open a map before reloading");
                }
            }
            if ui.menu_item_with_shortcut("Save File", "Ctrl+S") {
                match document.as_mut() {
                    Some(document) => save(document, &mut notice),
                    None => notice.show("Open a map before saving"),
                }
            }
            ui.separator();
            if ui.menu_item("Quit") {
                exit.write(AppExit::Success);
            }
        });
        // A menu-bar action, deliberately without a submenu.
        if ui.menu_item("Settings") {
            state.settings_open = true;
        }
    });
    settings_window(
        ui,
        &mut state.settings_open,
        &mut style,
        &mut selection_style,
    );
    unsaved_changes_dialog(ui, &mut open_request, &mut open_confirmation);
    prop_transform_window(
        ui,
        &mut state,
        &selected,
        &props,
        document.as_mut(),
        &mut snap,
        &mut model_browser,
    );
    model_browser_window(ui, &mut state, &mut model_browser);
    controls_window(ui);
    notice_window(ui, &notice.message);
}

fn prop_transform_window(
    ui: &Ui,
    state: &mut EditorUiState,
    selected: &SelectedProp,
    props: &Query<(&LifMapNodeIndex, &Transform, &LifPropRef), With<LifPropRef>>,
    document: Option<&mut ResMut<EditorDocument>>,
    snap: &mut TransformSnap,
    model_browser: &mut ModelBrowser,
) {
    ui.window("Prop Inspector")
        .position([18.0, 320.0], Condition::FirstUseEver)
        .flags(WindowFlags::ALWAYS_AUTO_RESIZE)
        .build(|| {
            ui.checkbox("Snap gizmo", &mut snap.enabled);
            ui.same_line();
            ui.input_float("Move step", &mut snap.translation_step);
            ui.input_float("Rotate step (degrees)", &mut snap.rotation_degrees);
            snap.translation_step = snap.translation_step.max(0.01);
            snap.rotation_degrees = snap.rotation_degrees.max(0.1);
            ui.separator();

            let Some(entity) = selected.0 else {
                ui.text_disabled("Select a prop to edit its transform.");
                return;
            };
            let Ok((node, transform, prop)) = props.get(entity) else {
                ui.text_disabled("The selected prop is no longer available.");
                return;
            };
            let Some(document) = document else {
                return;
            };
            ui.text(format!(
                "Node: {}",
                document
                    .0
                    .prop_node_name(node.0)
                    .ok()
                    .flatten()
                    .unwrap_or("(unnamed)")
            ));
            ui.text_wrapped(format!("Model: {}", prop.0.model));
            ui.text(format!("Animation type: {}", prop.0.anim_type));
            ui.text(format!("Animation speed: {:.3}", prop.0.anim_speed));
            if state.model_node != Some(node.0) {
                state.model_node = Some(node.0);
                state.model_path = prop.0.model.clone();
            }
            ui.input_text("Replacement model", &mut state.model_path)
                .build();
            ui.same_line();
            if ui.button("Browse Models") {
                model_browser.open = true;
                start_model_scan(model_browser);
            }
            if ui.button("Replace Model") {
                if let Err(error) = document.0.replace_prop_model(node.0, &state.model_path) {
                    error!("replacing prop model: {error:#}");
                }
            }
            ui.separator();
            ui.text("Transform");
            let mut translation = transform.translation.to_array();
            let (yaw, pitch, roll) = transform.rotation.to_euler(EulerRot::YXZ);
            let mut rotation = [yaw.to_degrees(), pitch.to_degrees(), roll.to_degrees()];
            let mut scale = transform.scale.to_array();
            let translation_changed = ui.input_float3("Position", &mut translation).build();
            let translation_finished = ui.is_item_deactivated_after_edit();
            let rotation_changed = ui.input_float3("Rotation", &mut rotation).build();
            let rotation_finished = ui.is_item_deactivated_after_edit();
            let scale_changed = ui.input_float3("Scale", &mut scale).build();
            let scale_finished = ui.is_item_deactivated_after_edit();
            let changed = translation_changed || rotation_changed || scale_changed;
            if changed {
                let rotation = Quat::from_euler(
                    EulerRot::YXZ,
                    rotation[0].to_radians(),
                    rotation[1].to_radians(),
                    rotation[2].to_radians(),
                );
                if let Err(error) = document.0.update_prop_transform(
                    node.0,
                    translation,
                    rotation.to_array(),
                    scale,
                ) {
                    error!("editing prop transform: {error:#}");
                }
            }
            if translation_finished || rotation_finished || scale_finished {
                document.0.commit_pending_prop_transform();
            }
            if ui.button("Reset Position") {
                reset_prop_transform(document.0.reset_prop_translation(node.0));
            }
            ui.same_line();
            if ui.button("Reset Rotation") {
                reset_prop_transform(document.0.reset_prop_rotation(node.0));
            }
            ui.same_line();
            if ui.button("Reset Scale") {
                reset_prop_transform(document.0.reset_prop_scale(node.0));
            }
        });
}

fn model_browser_window(ui: &Ui, state: &mut EditorUiState, browser: &mut ModelBrowser) {
    if !browser.open {
        return;
    }
    let mut open = browser.open;
    let mut selected = None;
    ui.window("Converted Models")
        .opened(&mut open)
        .position([420.0, 80.0], Condition::FirstUseEver)
        .size([560.0, 430.0], Condition::FirstUseEver)
        .build(|| {
            if model_scan_active(browser) {
                ui.text("Scanning converted models...");
                return;
            }
            ui.input_text("Search", &mut browser.query).build();
            let query = browser.query.to_lowercase();
            let match_count = browser
                .models
                .iter()
                .filter(|model| query.is_empty() || model.to_lowercase().contains(&query))
                .count();
            ui.child_window("Model results")
                .size([0.0, 350.0])
                .build(ui, || {
                    for model in browser
                        .models
                        .iter()
                        .filter(|model| query.is_empty() || model.to_lowercase().contains(&query))
                        .take(200)
                    {
                        if ui.selectable(model) {
                            selected = Some(model.clone());
                        }
                    }
                });
            if match_count > 200 {
                ui.text_disabled("Showing the first 200 matching models. Refine the search.");
            }
        });
    browser.open = open;
    if let Some(model) = selected {
        state.model_path = model;
        browser.open = false;
    }
}

fn reset_prop_transform(result: anyhow::Result<()>) {
    if let Err(error) = result {
        error!("resetting prop transform: {error:#}");
    }
}

fn unsaved_changes_dialog(
    ui: &Ui,
    request: &mut MapOpenRequest,
    confirmation: &mut MapOpenConfirmation,
) {
    if confirmation.0.is_none() {
        return;
    }
    ui.open_popup("Unsaved changes");
    ui.modal_popup("Unsaved changes", || {
        ui.text("The current map has unsaved changes.");
        ui.text("Discard them and reload the selected map?");
        if ui.button("Discard Changes") {
            confirm_map_open(request, confirmation);
            ui.close_current_popup();
        }
        ui.same_line();
        if ui.button("Cancel") {
            cancel_map_open(confirmation);
            ui.close_current_popup();
        }
    });
}

fn settings_window(
    ui: &Ui,
    open: &mut bool,
    style: &mut ResMut<TerrainGridStyle>,
    selection_style: &mut ResMut<SelectionOutlineStyle>,
) {
    if !*open {
        return;
    }
    ui.window("Settings")
        .opened(open)
        .position([18.0, 42.0], Condition::FirstUseEver)
        .size([380.0, 280.0], Condition::FirstUseEver)
        .build(|| {
            if ui.collapsing_header("Viewport", TreeNodeFlags::DEFAULT_OPEN) {
                ui.text("Terrain grid");
                // Work on copies: borrowing the resource mutably every frame would
                // rebuild the grid even when no widget has actually changed.
                let mut width = style.line_width;
                if ui.slider("Line thickness", 0.02, 0.3, &mut width) {
                    style.line_width = width;
                }
                let mut color = style.color;
                if ui.color_edit3("Line color", &mut color) {
                    style.color = color;
                }
            }
            if ui.collapsing_header("Selection", TreeNodeFlags::DEFAULT_OPEN) {
                let mut color = selection_style.color;
                if ui.color_edit3("Border color", &mut color) {
                    selection_style.color = color;
                }
                let mut padding = selection_style.padding;
                if ui.slider("Border padding", 0.0, 50.0, &mut padding) {
                    selection_style.padding = padding;
                }
            }
            ui.text_disabled("Changes apply immediately (this session only).");
        });
}

fn controls_window(ui: &Ui) {
    ui.window("Controls")
        .position(
            [ui.io().display_size()[0] - 340.0, 42.0],
            Condition::FirstUseEver,
        )
        .flags(WindowFlags::ALWAYS_AUTO_RESIZE)
        .build(|| {
            ui.text("Camera");
            ui.text("WASD - move / Space, Left Alt - up, down");
            ui.text("RMB - look / Wheel - speed / P - grid");
            ui.separator();
            ui.text("Collision");
            ui.text("LMB / arrows - select cell");
            ui.text("F - toggle walkability");
            ui.text("Ctrl+Z / Ctrl+Y - undo / redo");
            ui.text("Ctrl+S - save");
            ui.separator();
            ui.text("Props");
            ui.text("Click a prop, drag gizmo axis");
            ui.text("I/K J/L U/O - move / Q/E - rotate");
            ui.text("G/R - gizmo mode");
            ui.text("Ctrl+D - duplicate / Delete - remove");
        });
}

fn notice_window(ui: &Ui, message: &str) {
    if message.is_empty() {
        return;
    }
    ui.window("Notice")
        .position([18.0, ui.io().display_size()[1] - 70.0], Condition::Always)
        .flags(
            WindowFlags::NO_TITLE_BAR
                | WindowFlags::NO_RESIZE
                | WindowFlags::NO_MOVE
                | WindowFlags::ALWAYS_AUTO_RESIZE
                | WindowFlags::NO_INPUTS,
        )
        .build(|| ui.text(message));
}
