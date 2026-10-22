use bevy::prelude::*;
use lifthrasir_map_editor::LifCollisionFlags;

use super::viewport::{CELL_SIZE, EditorCamera};
use crate::application::{EditorDocument, EditorNotice, EditorSystems};

const COLLISION_OVERLAY_LIFT: f32 = 1.0;

#[derive(Component)]
struct CollisionCursor;

#[derive(Resource, Default)]
struct CursorCell {
    x: u32,
    y: u32,
}

#[derive(Resource, Default)]
struct CollisionBrush {
    radius: u32,
}

#[derive(Component)]
struct CollisionOverlay {
    center: Option<(u32, u32)>,
}

#[derive(Resource)]
struct OverlayAssets {
    mesh: Handle<Mesh>,
    walkable: Handle<StandardMaterial>,
    blocked: Handle<StandardMaterial>,
}

pub(super) struct CollisionEditorPlugin;

impl Plugin for CollisionEditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CursorCell>()
            .init_resource::<CollisionBrush>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (
                    move_cursor,
                    select_cursor_with_mouse,
                    adjust_brush_radius,
                    toggle_cursor_cell,
                )
                    .chain()
                    .in_set(EditorSystems::Edit)
                    .run_if(resource_exists::<EditorDocument>),
            )
            .add_systems(
                Update,
                (sync_cursor, refresh_collision_overlay)
                    .in_set(EditorSystems::Present)
                    .run_if(resource_exists::<EditorDocument>),
            );
    }
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        CollisionCursor,
        Mesh3d(meshes.add(Cuboid::new(CELL_SIZE, CELL_SIZE, CELL_SIZE))),
        MeshMaterial3d(materials.add(Color::srgb(1.0, 0.1, 0.1))),
        Transform::from_xyz(2.5, 2.5, -2.5),
    ));
    commands.insert_resource(OverlayAssets {
        mesh: meshes.add(Cuboid::new(4.7, 0.15, 4.7)),
        walkable: materials.add(overlay_material(Color::srgb(0.1, 0.95, 0.2))),
        blocked: materials.add(overlay_material(Color::srgb(1.0, 0.08, 0.08))),
    });
    commands.spawn((
        CollisionOverlay { center: None },
        Transform::default(),
        Visibility::Visible,
    ));
}

fn overlay_material(color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        unlit: true,
        ..default()
    }
}

fn toggle_cursor_cell(
    keys: Res<ButtonInput<KeyCode>>,
    mut document: ResMut<EditorDocument>,
    cursor: Res<CursorCell>,
    brush: Res<CollisionBrush>,
    mut notice: ResMut<EditorNotice>,
) {
    if !keys.just_pressed(KeyCode::KeyF) {
        return;
    }
    let Some(cell) = document.0.collision_cell(cursor.x, cursor.y) else {
        return;
    };
    // Paint the inverse of the centre cell's current state across the brush.
    let walkable = !cell.flags.contains(LifCollisionFlags::WALKABLE);
    if let Err(error) =
        document
            .0
            .set_collision_brush_walkable(cursor.x, cursor.y, brush.radius, walkable)
    {
        error!("editing collision: {error:#}");
    } else {
        let diameter = brush.radius * 2 + 1;
        notice.show(format!(
            "Painted {diameter}x{diameter} collision brush: {}",
            if walkable { "walkable" } else { "blocked" }
        ));
    }
}

fn adjust_brush_radius(keys: Res<ButtonInput<KeyCode>>, mut brush: ResMut<CollisionBrush>) {
    if keys.just_pressed(KeyCode::BracketLeft) {
        brush.radius = brush.radius.saturating_sub(1);
    }
    if keys.just_pressed(KeyCode::BracketRight) {
        brush.radius = (brush.radius + 1).min(16);
    }
}

fn move_cursor(
    keys: Res<ButtonInput<KeyCode>>,
    document: Res<EditorDocument>,
    mut cursor: ResMut<CursorCell>,
) {
    let collision = document.0.editor.collision;
    if keys.just_pressed(KeyCode::ArrowLeft) {
        cursor.x = cursor.x.saturating_sub(1);
    }
    if keys.just_pressed(KeyCode::ArrowRight) {
        cursor.x = (cursor.x + 1).min(collision.width - 1);
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        cursor.y = cursor.y.saturating_sub(1);
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        cursor.y = (cursor.y + 1).min(collision.height - 1);
    }
}

fn select_cursor_with_mouse(
    buttons: Res<ButtonInput<MouseButton>>,
    mut cursor_events: MessageReader<CursorMoved>,
    document: Res<EditorDocument>,
    mut cursor: ResMut<CursorCell>,
    cameras: Query<(&Camera, &GlobalTransform), With<EditorCamera>>,
) {
    let Some(position) = cursor_events.read().last().map(|event| event.position) else {
        return;
    };
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Ok((camera, camera_transform)) = cameras.single() else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(camera_transform, position) else {
        return;
    };
    if ray.direction.y.abs() < f32::EPSILON {
        return;
    }
    let distance = -ray.origin.y / ray.direction.y;
    if distance < 0.0 {
        return;
    }
    let hit = ray.origin + *ray.direction * distance;
    let (x, y) = (
        (hit.x / CELL_SIZE).floor() as i32,
        (-hit.z / CELL_SIZE).floor() as i32,
    );
    let collision = document.0.editor.collision;
    if x < 0 || y < 0 || x >= collision.width as i32 || y >= collision.height as i32 {
        return;
    }
    cursor.x = x as u32;
    cursor.y = y as u32;
}

/// Document/history synchronization is separate from input, including undo of
/// a collision edit while a prop is selected.
fn sync_cursor(
    document: Res<EditorDocument>,
    cursor: Res<CursorCell>,
    mut markers: Query<&mut Transform, With<CollisionCursor>>,
) {
    if !document.is_changed() && !cursor.is_changed() {
        return;
    }
    let Some(cell) = document.0.collision_cell(cursor.x, cursor.y) else {
        return;
    };
    let Ok(mut transform) = markers.single_mut() else {
        return;
    };
    transform.translation = collision_cell_position(cursor.x, cursor.y, cell, 0.0);
}

fn refresh_collision_overlay(
    mut commands: Commands,
    document: Res<EditorDocument>,
    cursor: Res<CursorCell>,
    assets: Res<OverlayAssets>,
    mut overlays: Query<(Entity, &mut CollisionOverlay, Option<&Children>)>,
) {
    let Ok((root, mut overlay, children)) = overlays.single_mut() else {
        return;
    };
    let center = (cursor.x, cursor.y);
    if overlay.center == Some(center) && !document.is_changed() {
        return;
    }
    if let Some(children) = children {
        for child in children.iter() {
            commands.entity(child).despawn();
        }
    }
    overlay.center = Some(center);

    const RADIUS: i32 = 8;
    let collision = document.0.editor.collision;
    for y in (center.1 as i32 - RADIUS)..=(center.1 as i32 + RADIUS) {
        for x in (center.0 as i32 - RADIUS)..=(center.0 as i32 + RADIUS) {
            if x < 0 || y < 0 || x >= collision.width as i32 || y >= collision.height as i32 {
                continue;
            }
            let Some(cell) = document.0.collision_cell(x as u32, y as u32) else {
                continue;
            };
            let material = if cell.flags.contains(LifCollisionFlags::WALKABLE) {
                assets.walkable.clone()
            } else {
                assets.blocked.clone()
            };
            commands.spawn((
                Mesh3d(assets.mesh.clone()),
                MeshMaterial3d(material),
                Visibility::Visible,
                Transform::from_translation(collision_cell_position(
                    x as u32,
                    y as u32,
                    cell,
                    COLLISION_OVERLAY_LIFT,
                )),
                ChildOf(root),
            ));
        }
    }
}

/// Converts a GAT/collision cell to the renderer's world convention. GAT is
/// +Y-down while the renderer is +Y-up, and the converted terrain has a fixed
/// 1.5-unit lift. Collision cells are five world units wide with +Y rows
/// running toward -Z.
fn collision_cell_position(
    x: u32,
    y: u32,
    cell: lifthrasir_map_editor::LifCollisionCell,
    lift: f32,
) -> Vec3 {
    Vec3::new(
        x as f32 * CELL_SIZE + CELL_SIZE * 0.5,
        1.5 - cell.height.iter().sum::<f32>() / 4.0 + lift,
        -(y as f32 * CELL_SIZE + CELL_SIZE * 0.5),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;

    #[test]
    fn empty_overlay_root_spawns_colored_cells_and_updates_after_edit() {
        let (_directory, document) = test_support::document();
        let walkable = Handle::<StandardMaterial>::default();
        let blocked = Handle::<StandardMaterial>::Uuid(
            bevy::asset::uuid::Uuid::from_u128(1),
            std::marker::PhantomData,
        );
        let mut app = App::new();
        app.insert_resource(EditorDocument(document))
            .init_resource::<CursorCell>()
            .insert_resource(OverlayAssets {
                mesh: Handle::default(),
                walkable: walkable.clone(),
                blocked: blocked.clone(),
            })
            .add_systems(Update, refresh_collision_overlay);
        let root = app
            .world_mut()
            .spawn((CollisionOverlay { center: None }, Transform::default()))
            .id();
        assert!(app.world().get::<Children>(root).is_none());

        app.update();

        let child = app.world().get::<Children>(root).expect("initial tiles")[0];
        assert_eq!(
            app.world()
                .get::<MeshMaterial3d<StandardMaterial>>(child)
                .unwrap()
                .0,
            walkable
        );
        app.world_mut()
            .resource_mut::<EditorDocument>()
            .0
            .set_collision_brush_walkable(0, 0, 0, false)
            .unwrap();
        app.update();
        let child = app.world().get::<Children>(root).expect("updated tiles")[0];
        assert_eq!(
            app.world()
                .get::<MeshMaterial3d<StandardMaterial>>(child)
                .unwrap()
                .0,
            blocked
        );
    }

    #[test]
    fn collision_cells_use_gat_world_coordinates_and_height_direction() {
        let cell = lifthrasir_map_editor::LifCollisionCell {
            height: [2.0, 4.0, 6.0, 8.0],
            flags: LifCollisionFlags::WALKABLE,
        };
        assert_eq!(
            collision_cell_position(2, 3, cell, 0.1),
            Vec3::new(12.5, -3.4, -17.5)
        );
    }

    #[test]
    fn f_paints_the_inverse_of_the_selected_cells_walkability() {
        let (_directory, document) = test_support::document();
        let mut app = App::new();
        app.insert_resource(EditorDocument(document))
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<EditorNotice>()
            .insert_resource(CursorCell { x: 0, y: 0 })
            .init_resource::<CollisionBrush>()
            .add_systems(Update, toggle_cursor_cell);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyF);

        app.update();

        assert!(
            !app.world()
                .resource::<EditorDocument>()
                .0
                .collision_cell(0, 0)
                .expect("cell")
                .flags
                .contains(LifCollisionFlags::WALKABLE)
        );
        assert!(
            app.world()
                .resource::<EditorNotice>()
                .message
                .contains("blocked")
        );
    }
}
