use bevy::prelude::*;
use lifthrasir_map_editor::LifCollisionFlags;

use super::viewport::{CELL_SIZE, EditorCamera};
use crate::application::{EditorDocument, EditorNotice, EditorSystems};

#[derive(Resource, Default)]
pub(super) struct CursorCell {
    pub(super) x: u32,
    pub(super) y: u32,
}

#[derive(Resource, Default)]
pub(super) struct CollisionBrush {
    pub(super) radius: u32,
}

pub(super) struct CollisionEditorPlugin;

impl Plugin for CollisionEditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CursorCell>()
            .init_resource::<CollisionBrush>()
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
            );
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

/// Converts a GAT/collision cell to the renderer's world convention. GAT is
/// +Y-down while the renderer is +Y-up, and the converted terrain has a fixed
/// 1.5-unit lift. Collision cells are five world units wide with +Y rows
/// running toward -Z.
#[cfg(test)]
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
