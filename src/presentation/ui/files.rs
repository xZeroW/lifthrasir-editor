//! Native file picking and document/scene replacement.
use bevy::{
    prelude::*,
    tasks::{IoTaskPool, Task, block_on, futures_lite::future::poll_once},
};

use crate::{
    application::{ActiveMapRoot, EditorDocument, EditorNotice, control_pressed},
    infrastructure::content::MapSceneRequest,
    map_asset_path,
    presentation::MapAssetPath,
};

#[derive(Resource, Default)]
pub(super) struct MapFilePicker(Option<Task<Option<std::path::PathBuf>>>);

/// A map selected for opening after its unsaved-change check has completed.
#[derive(Resource, Default)]
pub(super) struct MapOpenRequest(pub(super) Option<std::path::PathBuf>);

/// A map whose open operation needs the user's confirmation because it would
/// discard the current document's in-memory edits.
#[derive(Resource, Default)]
pub(super) struct MapOpenConfirmation(pub(super) Option<std::path::PathBuf>);

pub(super) fn start_map_file_picker(picker: &mut MapFilePicker) {
    if picker.0.is_some() {
        return;
    }
    picker.0 = Some(IoTaskPool::get().spawn(async {
        rfd::AsyncFileDialog::new()
            .set_directory("assets/data/maps")
            .add_filter("Lifthrasir maps", &["glb"])
            .pick_file()
            .await
            .map(|file| file.path().to_path_buf())
    }));
}

pub(super) fn open_file_shortcut(
    keys: Res<ButtonInput<KeyCode>>,
    mut picker: ResMut<MapFilePicker>,
) {
    if control_pressed(&keys) && keys.just_pressed(KeyCode::KeyO) {
        start_map_file_picker(&mut picker);
    }
}

pub(super) fn reload_file_shortcut(
    keys: Res<ButtonInput<KeyCode>>,
    document: Option<Res<EditorDocument>>,
    mut request: ResMut<MapOpenRequest>,
    mut confirmation: ResMut<MapOpenConfirmation>,
    mut notice: ResMut<EditorNotice>,
) {
    if !control_pressed(&keys) || !keys.just_pressed(KeyCode::KeyR) {
        return;
    }
    let Some(document) = document else {
        notice.show("Open a map before reloading");
        return;
    };
    request_map_open(
        document.0.source_path.clone(),
        Some(&document),
        &mut request,
        &mut confirmation,
    );
}

pub(super) fn poll_map_file_picker(
    mut picker: ResMut<MapFilePicker>,
    document: Option<Res<EditorDocument>>,
    mut request: ResMut<MapOpenRequest>,
    mut confirmation: ResMut<MapOpenConfirmation>,
) {
    let Some(task) = picker.0.as_mut() else {
        return;
    };
    let Some(path) = block_on(poll_once(task)) else {
        return;
    };
    picker.0 = None;
    let Some(path) = path else { return };
    request_map_open(path, document.as_deref(), &mut request, &mut confirmation);
}

pub(super) fn request_map_open(
    path: std::path::PathBuf,
    document: Option<&EditorDocument>,
    request: &mut MapOpenRequest,
    confirmation: &mut MapOpenConfirmation,
) {
    if document.is_some_and(|document| document.0.is_dirty()) {
        confirmation.0 = Some(path);
    } else {
        request.0 = Some(path);
    }
}

pub(super) fn confirm_map_open(
    request: &mut MapOpenRequest,
    confirmation: &mut MapOpenConfirmation,
) {
    request.0 = confirmation.0.take();
}

pub(super) fn cancel_map_open(confirmation: &mut MapOpenConfirmation) {
    confirmation.0 = None;
}

pub(super) fn open_requested_map(
    mut commands: Commands,
    mut request: ResMut<MapOpenRequest>,
    document: Option<ResMut<EditorDocument>>,
    mut map_path: ResMut<MapAssetPath>,
    mut requests: Query<(Entity, &MapSceneRequest, &mut Visibility), With<ActiveMapRoot>>,
    mut notice: ResMut<EditorNotice>,
) {
    let Some(path) = request.0.take() else {
        return;
    };
    let asset_path = match map_asset_path(&path) {
        Ok(path) => path,
        Err(error) => {
            notice.show(format!("Open failed: {error}"));
            return;
        }
    };
    let replacement = match lifthrasir_map_editor::MapDocument::open(&path) {
        Ok(document) => document,
        Err(error) => {
            notice.show(format!("Open failed: {error}"));
            return;
        }
    };
    if let Some(mut document) = document {
        *document = EditorDocument(replacement);
    } else {
        commands.insert_resource(EditorDocument(replacement));
    }
    map_path.0 = Some(asset_path.clone());
    let scene_path = format!("ro://{asset_path}");
    if let Ok((root, request, mut visibility)) = requests.single_mut() {
        if request.path == scene_path {
            notice.show(format!("Opened {}", path.display()));
            return;
        }
        // Keep the old root alive: Bevy's world-asset spawner may still have
        // nested prop instances queued against it. Removing/replacing it can
        // otherwise make those queued instances parent to a despawned entity.
        *visibility = Visibility::Hidden;
        commands.entity(root).remove::<ActiveMapRoot>();
    }
    commands.spawn((ActiveMapRoot, MapSceneRequest { path: scene_path }));
    notice.show(format!("Opened {}", path.display()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;

    #[test]
    fn dirty_documents_require_confirmation_before_replacement() {
        let (_directory, mut document) = test_support::document();
        document
            .set_prop_transform(0, [1.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0], [1.0; 3])
            .unwrap();
        let path = document.source_path.clone();
        let mut request = MapOpenRequest::default();
        let mut confirmation = MapOpenConfirmation::default();

        request_map_open(
            path.clone(),
            Some(&EditorDocument(document)),
            &mut request,
            &mut confirmation,
        );
        assert!(request.0.is_none());
        assert_eq!(confirmation.0, Some(path));

        confirm_map_open(&mut request, &mut confirmation);
        assert!(confirmation.0.is_none());
        assert!(request.0.is_some());
    }
}
