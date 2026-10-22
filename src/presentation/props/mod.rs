mod gizmo;
mod keyboard;
mod selection;

use bevy::prelude::*;

use crate::{
    application::{EditorDocument, EditorNotice, EditorSystems, control_pressed},
    infrastructure::content::{LifMapNodeIndex, LifPropRef, PropSceneContext},
};
use lifthrasir_runtime::world::props::MapModel;

#[derive(Resource, Default)]
pub(crate) struct SelectedProp(pub(crate) Option<Entity>);

#[derive(Resource)]
pub(crate) struct TransformSnap {
    pub enabled: bool,
    pub translation_step: f32,
    pub rotation_degrees: f32,
}

impl Default for TransformSnap {
    fn default() -> Self {
        Self {
            enabled: false,
            translation_step: 5.0,
            rotation_degrees: 15.0,
        }
    }
}

#[derive(Resource)]
pub(crate) struct SelectionOutlineStyle {
    pub color: [f32; 3],
    pub padding: f32,
}

impl Default for SelectionOutlineStyle {
    fn default() -> Self {
        Self {
            color: [0.95, 0.1, 0.1],
            padding: 1.0,
        }
    }
}

pub(super) struct PropEditorPlugin;

impl Plugin for PropEditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SelectedProp>()
            .init_resource::<TransformSnap>()
            .init_resource::<SelectionOutlineStyle>()
            .add_plugins((selection::PropSelectionPlugin, gizmo::PropGizmoPlugin))
            .add_systems(
                Update,
                (
                    keyboard::move_selected_prop,
                    duplicate_selected_prop,
                    delete_selected_prop,
                )
                    .chain()
                    .in_set(EditorSystems::Edit)
                    .run_if(resource_exists::<EditorDocument>),
            )
            .add_systems(
                Update,
                sync_document_props
                    .in_set(EditorSystems::Present)
                    .run_if(resource_exists::<EditorDocument>),
            );
    }
}

fn duplicate_selected_prop(
    keys: Res<ButtonInput<KeyCode>>,
    selected: Res<SelectedProp>,
    mut document: ResMut<EditorDocument>,
    props: Query<(&LifMapNodeIndex, &LifPropRef, &Transform), With<LifPropRef>>,
    mut commands: Commands,
    mut notice: ResMut<EditorNotice>,
) {
    if !keys.just_pressed(KeyCode::KeyD) || !control_pressed(&keys) {
        return;
    }
    let Some(entity) = selected.0 else { return };
    let Ok((node, prop, transform)) = props.get(entity) else {
        return;
    };
    match document.0.duplicate_prop(node.0) {
        Ok(index) => {
            let mut duplicate_transform = *transform;
            duplicate_transform.translation.x += 20.0;
            commands.spawn((
                LifPropRef(prop.0.clone()),
                LifMapNodeIndex(index),
                PropSceneContext::default(),
                duplicate_transform,
                Visibility::Inherited,
            ));
            notice.show(format!("Duplicated prop (node {index})"));
        }
        Err(error) => notice.show(format!("Duplicate failed: {error}")),
    }
}

fn delete_selected_prop(
    keys: Res<ButtonInput<KeyCode>>,
    mut document: ResMut<EditorDocument>,
    selected: Res<SelectedProp>,
    mut props: Query<(&LifMapNodeIndex, &mut Visibility), With<LifPropRef>>,
    mut notice: ResMut<EditorNotice>,
) {
    if !keys.just_pressed(KeyCode::Delete) {
        return;
    }
    let Some(entity) = selected.0 else { return };
    let Ok((node, mut visibility)) = props.get_mut(entity) else {
        return;
    };
    if let Err(error) = document.0.set_prop_deleted(node.0, true) {
        error!("deleting prop: {error:#}");
        notice.show(format!("Delete failed: {error}"));
        return;
    }
    *visibility = Visibility::Hidden;
    notice.show("Deleted prop");
}

/// Scene synchronization belongs to presentation, not the history command.
/// Synchronize every prop so undo does not depend on the current selection.
fn sync_document_props(
    mut commands: Commands,
    document: Res<EditorDocument>,
    mut props: Query<
        (
            Entity,
            &mut Transform,
            &LifMapNodeIndex,
            &mut Visibility,
            &mut LifPropRef,
            Option<&Children>,
        ),
        With<LifPropRef>,
    >,
) {
    if !document.is_changed() {
        return;
    }
    for (entity, mut transform, node, mut visibility, mut prop, children) in &mut props {
        if let Ok(model) = document.0.prop_model_path(node.0)
            && prop.0.model != model
        {
            prop.0.model = model.to_owned();
            if let Some(children) = children {
                for child in children.iter() {
                    commands.entity(child).despawn();
                }
            }
            commands.entity(entity).remove::<MapModel>();
        }
        if let Ok((translation, rotation, scale)) = document.0.prop_transform(node.0) {
            transform.translation = Vec3::from(translation);
            transform.rotation = Quat::from_array(rotation);
            transform.scale = Vec3::from(scale);
        }
        *visibility = if document.0.prop_deleted(node.0) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{application::EditorSessionPlugin, test_support};
    use lifthrasir_data::lif::LifProp;

    #[test]
    fn duplicated_props_can_be_undone_redone_deleted_and_restored() {
        let (_directory, document) = test_support::document();
        let mut app = App::new();
        app.insert_resource(EditorDocument(document))
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<SelectedProp>()
            .add_plugins(EditorSessionPlugin)
            .add_systems(
                Update,
                (duplicate_selected_prop, delete_selected_prop)
                    .chain()
                    .in_set(EditorSystems::Edit),
            )
            .add_systems(Update, sync_document_props.in_set(EditorSystems::Present));
        let original = app
            .world_mut()
            .spawn((
                LifPropRef(LifProp {
                    model: "ro://models/tree.glb".into(),
                    anim_type: 0,
                    anim_speed: 1.0,
                }),
                LifMapNodeIndex(0),
                Transform::default(),
                Visibility::Inherited,
            ))
            .id();
        app.world_mut().resource_mut::<SelectedProp>().0 = Some(original);
        shortcut(&mut app, Some(KeyCode::ControlLeft), KeyCode::KeyD);
        let (duplicate, index) = app
            .world_mut()
            .query::<(Entity, &LifMapNodeIndex)>()
            .iter(app.world())
            .find(|(entity, _)| *entity != original)
            .map(|(entity, node)| (entity, node.0))
            .unwrap();
        assert_eq!(
            app.world().get::<Visibility>(duplicate),
            Some(&Visibility::Inherited)
        );
        assert_eq!(
            app.world().get::<Transform>(duplicate).unwrap().translation,
            Vec3::X * 20.0
        );

        shortcut(&mut app, Some(KeyCode::ControlLeft), KeyCode::KeyZ);
        assert!(
            app.world()
                .resource::<EditorDocument>()
                .0
                .prop_deleted(index)
        );
        assert_eq!(
            app.world().get::<Visibility>(duplicate),
            Some(&Visibility::Hidden)
        );
        shortcut(&mut app, Some(KeyCode::ControlRight), KeyCode::KeyY);
        assert!(
            !app.world()
                .resource::<EditorDocument>()
                .0
                .prop_deleted(index)
        );
        assert_eq!(
            app.world().get::<Visibility>(duplicate),
            Some(&Visibility::Inherited)
        );

        // Selecting the duplicate, rather than its source, must allow delete.
        app.world_mut().resource_mut::<SelectedProp>().0 = Some(duplicate);
        shortcut(&mut app, None, KeyCode::Delete);
        assert!(
            app.world()
                .resource::<EditorDocument>()
                .0
                .prop_deleted(index)
        );
        assert_eq!(
            app.world().get::<Visibility>(duplicate),
            Some(&Visibility::Hidden)
        );
        shortcut(&mut app, Some(KeyCode::ControlLeft), KeyCode::KeyZ);
        assert!(
            !app.world()
                .resource::<EditorDocument>()
                .0
                .prop_deleted(index)
        );
        assert_eq!(
            app.world().get::<Visibility>(duplicate),
            Some(&Visibility::Inherited)
        );
        shortcut(&mut app, Some(KeyCode::ControlLeft), KeyCode::KeyY);
        assert!(
            app.world()
                .resource::<EditorDocument>()
                .0
                .prop_deleted(index)
        );
        assert_eq!(
            app.world().get::<Visibility>(duplicate),
            Some(&Visibility::Hidden)
        );
        assert_eq!(
            app.world().get::<Visibility>(original),
            Some(&Visibility::Inherited)
        );
    }

    fn shortcut(app: &mut App, control: Option<KeyCode>, key: KeyCode) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        if let Some(control) = control {
            keys.press(control);
        }
        keys.press(key);
        app.update();
    }
}
