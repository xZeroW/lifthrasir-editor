use bevy::{camera::primitives::Aabb, prelude::*};

use super::{SelectedProp, SelectionOutlineStyle};
use crate::{application::EditorSystems, infrastructure::content::LifPropRef};

pub(super) struct PropSelectionPlugin;

impl Plugin for PropSelectionPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(select_prop)
            .add_systems(Update, draw_prop_selection.in_set(EditorSystems::Present));
    }
}

/// Meshes are nested beneath the map prop, so walk up the owning hierarchy.
fn select_prop(
    click: On<Pointer<Click>>,
    parents: Query<&ChildOf>,
    props: Query<(), With<LifPropRef>>,
    mut selected: ResMut<SelectedProp>,
    capture: Option<Res<dear_imgui_bevy::prelude::ImguiInputCapture>>,
) {
    if crate::presentation::ui::pointer_captured(capture.as_deref()) {
        return;
    }
    let mut entity = click.entity;
    loop {
        if props.contains(entity) {
            selected.0 = Some(entity);
            return;
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    }
}

fn draw_prop_selection(
    selected: Res<SelectedProp>,
    props: Query<(), With<LifPropRef>>,
    children: Query<&Children>,
    bounds: Query<(&GlobalTransform, &Aabb)>,
    style: Res<SelectionOutlineStyle>,
    mut gizmos: Gizmos,
) {
    let Some(entity) = selected.0 else {
        return;
    };
    if !props.contains(entity) {
        return;
    }
    let Some((minimum, maximum)) = prop_bounds(entity, &children, &bounds) else {
        return;
    };
    let padding = Vec3::splat(style.padding.max(0.0));
    let size = (maximum - minimum) + padding * 2.0;
    gizmos.cube(
        Transform::from_translation((minimum + maximum) * 0.5).with_scale(size),
        Color::srgb(style.color[0], style.color[1], style.color[2]),
    );
}

/// Calculates a world-aligned bounds box from every renderable mesh nested
/// below the selected prop. This follows the model instead of using the prop
/// root's pivot, which is often on the terrain below its visible geometry.
fn prop_bounds(
    prop: Entity,
    children: &Query<&Children>,
    bounds: &Query<(&GlobalTransform, &Aabb)>,
) -> Option<(Vec3, Vec3)> {
    let mut minimum = Vec3::splat(f32::INFINITY);
    let mut maximum = Vec3::splat(f32::NEG_INFINITY);
    let mut found = false;
    for entity in std::iter::once(prop).chain(children.iter_descendants(prop)) {
        let Ok((transform, aabb)) = bounds.get(entity) else {
            continue;
        };
        let center = Vec3::from(aabb.center);
        let extent = Vec3::from(aabb.half_extents);
        for x in [-extent.x, extent.x] {
            for y in [-extent.y, extent.y] {
                for z in [-extent.z, extent.z] {
                    let point = transform.transform_point(center + Vec3::new(x, y, z));
                    minimum = minimum.min(point);
                    maximum = maximum.max(point);
                    found = true;
                }
            }
        }
    }
    found.then_some((minimum, maximum))
}
