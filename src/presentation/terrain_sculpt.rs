//! Terrain brush commands and document-to-render-mesh synchronization.
use bevy::{camera::primitives::Aabb, prelude::*};

use super::collision::{CollisionBrush, CursorCell};
use crate::application::{
    ActiveMapRoot, EditorDocument, EditorNotice, EditorSystems, control_pressed,
};

pub(super) struct TerrainSculptPlugin;

impl Plugin for TerrainSculptPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            sculpt_terrain
                .in_set(EditorSystems::Edit)
                .run_if(resource_exists::<EditorDocument>),
        )
        .add_systems(
            Update,
            sync_terrain_meshes
                .after(EditorSystems::Save)
                .before(EditorSystems::Present)
                .run_if(resource_exists::<EditorDocument>),
        );
    }
}

fn sculpt_terrain(
    keys: Res<ButtonInput<KeyCode>>,
    cursor: Res<CursorCell>,
    brush: Res<CollisionBrush>,
    mut document: ResMut<EditorDocument>,
    mut notice: ResMut<EditorNotice>,
) {
    if control_pressed(&keys) {
        return;
    }
    let delta = if keys.just_pressed(KeyCode::PageUp) {
        1.0
    } else if keys.just_pressed(KeyCode::PageDown) {
        -1.0
    } else {
        return;
    };
    match document
        .0
        .sculpt_terrain(cursor.x, cursor.y, brush.radius, delta)
    {
        Ok(0) => notice.show("No terrain vertices in brush; increase the brush size"),
        Ok(count) => notice.show(format!(
            "Sculpted {count} terrain vertices by {delta:+} (collision heights unchanged)"
        )),
        Err(error) => notice.show(format!("Terrain sculpt failed: {error:#}")),
    }
}

#[derive(Component)]
struct EditedTerrainMesh {
    original: Handle<Mesh>,
    mesh_index: usize,
    primitive_index: usize,
    revision: u64,
}

fn sync_terrain_meshes(
    mut commands: Commands,
    document: Res<EditorDocument>,
    roots: Query<Entity, With<ActiveMapRoot>>,
    children: Query<&Children>,
    names: Query<&Name>,
    mut entities: Query<(Entity, &mut Mesh3d, Option<&mut EditedTerrainMesh>)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Ok(root) = roots.single() else { return };
    let Some(terrain_root) = children.iter_descendants(root).find(|&entity| {
        names
            .get(entity)
            .is_ok_and(|name| name.as_str() == "terrain")
    }) else {
        return;
    };
    for entity in std::iter::once(terrain_root).chain(children.iter_descendants(terrain_root)) {
        let Ok((entity, mut handle, binding)) = entities.get_mut(entity) else {
            continue;
        };
        let Some(primitives) = document.0.terrain_primitives() else {
            if let Some(binding) = binding {
                handle.0 = binding.original.clone();
                commands
                    .entity(entity)
                    .remove::<(EditedTerrainMesh, Aabb)>();
            }
            continue;
        };
        let (mesh_index, primitive_index) = if let Some(binding) = binding.as_ref() {
            if binding.revision == document.0.terrain_revision() {
                continue;
            }
            (binding.mesh_index, binding.primitive_index)
        } else {
            let Some(label) = handle.0.path().and_then(|path| path.label()) else {
                continue;
            };
            let Some((mesh, primitive)) = label
                .strip_prefix("Mesh")
                .and_then(|label| label.split_once("/Primitive"))
            else {
                continue;
            };
            let (Ok(mesh), Ok(primitive)) = (mesh.parse::<usize>(), primitive.parse::<usize>())
            else {
                continue;
            };
            (mesh, primitive)
        };
        let Some(primitive) = primitives.iter().find(|primitive| {
            primitive.mesh_index == mesh_index && primitive.primitive_index == primitive_index
        }) else {
            continue;
        };
        let Some(source) = meshes.get(&handle.0) else {
            continue;
        };
        let mut mesh = source.clone();
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, primitive.positions.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, primitive.normals.clone());
        let original = binding
            .as_ref()
            .map(|binding| binding.original.clone())
            .unwrap_or_else(|| handle.0.clone());
        handle.0 = meshes.add(mesh);
        commands
            .entity(entity)
            .remove::<Aabb>()
            .insert(EditedTerrainMesh {
                original,
                mesh_index,
                primitive_index,
                revision: document.0.terrain_revision(),
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_systems_initialize_without_a_rendered_scene() {
        let (_directory, document) = crate::test_support::document();
        let mut app = App::new();
        app.insert_resource(EditorDocument(document))
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<CursorCell>()
            .init_resource::<CollisionBrush>()
            .init_resource::<EditorNotice>()
            .init_resource::<Assets<Mesh>>()
            .add_plugins(TerrainSculptPlugin);
        app.world_mut().spawn(ActiveMapRoot);
        app.update();
        assert!(!app.world().resource::<EditorDocument>().0.is_dirty());
    }
}
