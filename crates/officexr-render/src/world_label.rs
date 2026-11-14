//! Screen-space text over a world point, as drei's `<Html>` draws
//! `PlayerNameTag.tsx`: a Bevy UI node on its view's camera, moved each
//! frame to where that camera sees the point.

use bevy::prelude::*;

use crate::views::View;

/// Kept `offset` (world space) above its [`LabelOf`] entity, while `shown`.
#[derive(Component)]
pub(crate) struct WorldLabel {
    pub offset: Vec3,
    pub shown: bool,
}

/// The entity a label follows; despawning it despawns the label.
#[derive(Component)]
#[relationship(relationship_target = Labels)]
pub(crate) struct LabelOf(pub Entity);

#[derive(Component)]
#[relationship_target(relationship = LabelOf, linked_spawn)]
pub(crate) struct Labels(Vec<Entity>);

pub(crate) fn label(
    text: String,
    font_size: f32,
    background: Color,
    anchor: Entity,
    offset: Vec3,
) -> impl Bundle {
    (
        WorldLabel {
            offset,
            shown: true,
        },
        LabelOf(anchor),
        Text::new(text),
        TextFont::from_font_size(font_size),
        Node {
            position_type: PositionType::Absolute,
            padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(background),
        Visibility::Hidden,
    )
}

/// Centres each label on its point in its anchor's view, hiding it when
/// it isn't shown or the point is off camera.
#[allow(clippy::type_complexity)]
pub(crate) fn place_labels(
    mut commands: Commands,
    anchors: Query<&GlobalTransform>,
    parents: Query<&ChildOf>,
    views: Query<&View>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    mut labels: Query<(
        Entity,
        &WorldLabel,
        &LabelOf,
        &mut Node,
        &mut Visibility,
        &ComputedNode,
        Has<UiTargetCamera>,
    )>,
) {
    for (entity, label, anchor, mut node, mut visibility, computed, targeted) in &mut labels {
        let Some((camera_entity, (camera, camera_transform))) = parents
            .iter_ancestors(anchor.0)
            .find_map(|e| views.get(e).ok())
            .and_then(|v| v.camera)
            .and_then(|c| Some((c, cameras.get(c).ok()?)))
        else {
            continue;
        };
        // Spawned before its view's camera.
        if !targeted {
            commands
                .entity(entity)
                .insert(UiTargetCamera(camera_entity));
        }
        let at = anchors.get(anchor.0).ok().and_then(|t| {
            camera
                .world_to_viewport(camera_transform, t.translation() + label.offset)
                .ok()
        });
        match at {
            Some(xy) if label.shown => {
                let half = computed.size() * computed.inverse_scale_factor() / 2.0;
                node.left = Val::Px(xy.x - half.x);
                node.top = Val::Px(xy.y - half.y);
                *visibility = Visibility::Inherited;
            }
            _ => *visibility = Visibility::Hidden,
        }
    }
}
