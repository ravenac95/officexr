//! Screen-space text over a world point, as drei's `<Html>` draws
//! `PlayerNameTag.tsx`: a Bevy UI node on its view's camera, moved each
//! frame to where that camera sees the point.

use bevy::prelude::*;

use crate::views::View;

/// Kept `offset` (world space) above its [`LabelOf`] entity, while `shown`.
#[derive(Component)]
pub(crate) struct WorldLabel {
    pub offset: Vec3,
    /// Logical pixels the label moves by once centred on its point.
    pub nudge: Vec2,
    pub shown: bool,
}

/// The entity a label follows; despawning it despawns the label.
#[derive(Component)]
#[relationship(relationship_target = Labels)]
pub(crate) struct LabelOf(pub Entity);

#[derive(Component)]
#[relationship_target(relationship = LabelOf, linked_spawn)]
pub(crate) struct Labels(Vec<Entity>);

/// What every label shares: it follows `anchor`, `offset` above it, and stays hidden until placed.
fn anchored(anchor: Entity, offset: Vec3, node: Node, background: Color) -> impl Bundle {
    (
        WorldLabel {
            offset,
            nudge: Vec2::ZERO,
            shown: true,
        },
        LabelOf(anchor),
        Node {
            position_type: PositionType::Absolute,
            ..node
        },
        BackgroundColor(background),
        Visibility::Hidden,
    )
}

pub(crate) fn label(
    text: String,
    font_size: f32,
    background: Color,
    anchor: Entity,
    offset: Vec3,
) -> impl Bundle {
    let node = Node {
        padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
        ..default()
    };
    (
        anchored(anchor, offset, node, background),
        Text::new(text),
        TextFont::from_font_size(font_size),
    )
}

/// Text on a rounded `background` pill, as the mission marker's caption.
pub(crate) fn pill(
    text: String,
    font_size: f32,
    background: Color,
    anchor: Entity,
    offset: Vec3,
) -> impl Bundle {
    let node = Node {
        padding: UiRect::axes(Val::Px(12.0), Val::Px(4.0)),
        border_radius: BorderRadius::MAX,
        ..default()
    };
    (
        anchored(anchor, offset, node, background),
        Text::new(text),
        TextFont::from_font_size(font_size),
    )
}

/// A round `badge_px` badge holding `icon` at `icon_px`.
pub(crate) fn icon_badge(
    icon: Handle<Image>,
    badge_px: f32,
    icon_px: f32,
    background: Color,
    anchor: Entity,
    offset: Vec3,
) -> impl Bundle {
    let node = Node {
        width: Val::Px(badge_px),
        height: Val::Px(badge_px),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        border_radius: BorderRadius::MAX,
        ..default()
    };
    (
        anchored(anchor, offset, node, background),
        children![(
            ImageNode::new(icon),
            Node {
                width: Val::Px(icon_px),
                height: Val::Px(icon_px),
                ..default()
            },
        )],
    )
}

/// Where a placed label is, in its view's logical pixels.
// ponytail: logical px equal the pointer's CSS px only for the primary window; an image-target view is CSS px times DPR at scale 1.
#[cfg_attr(
    not(feature = "picking"),
    expect(dead_code, reason = "only the ping bell hit-tests labels")
)]
pub(crate) fn rect(node: &Node, computed: &ComputedNode) -> Option<Rect> {
    let (Val::Px(left), Val::Px(top)) = (node.left, node.top) else {
        return None;
    };
    let size = computed.size() * computed.inverse_scale_factor();
    Some(Rect::from_corners(
        Vec2::new(left, top),
        Vec2::new(left, top) + size,
    ))
}

/// The camera of the view `anchor` is in, once it has one.
fn view_camera(anchor: Entity, parents: &Query<&ChildOf>, views: &Query<&View>) -> Option<Entity> {
    parents
        .iter_ancestors(anchor)
        .find_map(|e| views.get(e).ok())
        .and_then(|v| v.camera)
}

/// Puts each new label on its view's camera before the UI first lays it
/// out, so it never draws, even for a frame, on another view's.
#[allow(clippy::type_complexity)]
pub(crate) fn target_labels(
    mut commands: Commands,
    labels: Query<(Entity, &LabelOf), (With<WorldLabel>, Without<UiTargetCamera>)>,
    parents: Query<&ChildOf>,
    views: Query<&View>,
) {
    for (label, anchor) in &labels {
        if let Some(camera) = view_camera(anchor.0, &parents, &views) {
            commands.entity(label).insert(UiTargetCamera(camera));
        }
    }
}

/// Centres each label on its point in its anchor's view, hiding it when
/// it isn't shown, the point is off camera, or it hasn't been laid out yet
/// (it would show off centre).
#[allow(clippy::type_complexity)]
pub(crate) fn place_labels(
    anchors: Query<&GlobalTransform>,
    parents: Query<&ChildOf>,
    views: Query<&View>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    mut labels: Query<
        (
            &WorldLabel,
            &LabelOf,
            &mut Node,
            &mut Visibility,
            &ComputedNode,
        ),
        With<UiTargetCamera>,
    >,
) {
    for (label, anchor, mut node, mut visibility, computed) in &mut labels {
        let Some((camera, camera_transform)) =
            view_camera(anchor.0, &parents, &views).and_then(|c| cameras.get(c).ok())
        else {
            continue;
        };
        let at = anchors.get(anchor.0).ok().and_then(|t| {
            camera
                .world_to_viewport(camera_transform, t.translation() + label.offset)
                .ok()
        });
        match at {
            Some(xy) if label.shown && computed.size() != Vec2::ZERO => {
                let half = computed.size() * computed.inverse_scale_factor() / 2.0;
                node.left = Val::Px(xy.x - half.x + label.nudge.x);
                node.top = Val::Px(xy.y - half.y + label.nudge.y);
                *visibility = Visibility::Inherited;
            }
            _ => *visibility = Visibility::Hidden,
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
    use bevy::camera::{
        CameraPlugin, CameraProjection, ComputedCameraValues, PerspectiveProjection,
        RenderTargetInfo,
    };
    use bevy::ui::{ComputedUiTargetCamera, UiScale, update::propagate_ui_target_cameras};

    use super::*;
    use crate::WorldDiff;
    use crate::tests::{bare_app, camera, scene};
    use crate::views::{ViewHub, ViewId, ViewKind, ViewMsg, ViewTarget, find_view};

    /// With the transforms, cameras and the UI's camera targeting the labels need.
    fn app() -> App {
        let mut app = bare_app();
        app.add_plugins((
            TransformPlugin,
            CameraPlugin,
            HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate),
        ))
        .init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>()
        .init_resource::<UiScale>()
        .add_systems(
            PostUpdate,
            propagate_ui_target_cameras.before(PropagateSet::<ComputedUiTargetCamera>::default()),
        );
        app
    }

    fn view_with_label(app: &mut App, text: &str) -> ViewId {
        let hub = app.world().resource::<ViewHub>().clone();
        let id = hub.create(ViewKind::World, |_| ViewTarget::Image(UVec2::new(64, 48)));
        let mut diff: WorldDiff =
            serde_json::from_str(include_str!("../examples/booper.scene.json")).unwrap();
        diff.kinds = Some(vec![]);
        diff.objects = scene(&[]).objects.into();
        hub.send(id, ViewMsg::World(Box::new(diff)));
        hub.send(id, ViewMsg::Camera(camera(0.0)));
        let label =
            serde_json::json!([{"tag": "l", "kind": "label", "at": [0, 0, 0], "text": text}]);
        hub.send(
            id,
            ViewMsg::Overlays(serde_json::from_value(label).unwrap()),
        );
        id
    }

    fn label_in(app: &mut App, id: ViewId) -> (Entity, Entity) {
        let view = find_view(app.world_mut(), id).unwrap();
        let camera = app.world().get::<View>(view).unwrap().camera.unwrap();
        let mut labels = app.world_mut().query::<(Entity, &LabelOf)>();
        let mut parents = app.world_mut().query::<&ChildOf>();
        let world = app.world();
        let label = labels
            .iter(world)
            .find(|(_, a)| parents.get(world, a.0).is_ok_and(|p| p.parent() == view))
            .unwrap()
            .0;
        (label, camera)
    }

    #[test]
    fn a_label_is_on_its_own_views_camera_from_its_first_frame() {
        let mut app = app();
        let a = view_with_label(&mut app, "A");
        let b = view_with_label(&mut app, "B");
        app.update();
        for id in [a, b] {
            let (label, camera) = label_in(&mut app, id);
            let target = app.world().get::<ComputedUiTargetCamera>(label);
            assert_eq!(target.and_then(|t| t.get()), Some(camera), "view {id}");
            assert_eq!(
                app.world().get::<Visibility>(label),
                Some(&Visibility::Hidden)
            );
        }
    }

    fn visibility(app: &App, label: Entity) -> Visibility {
        *app.world().get::<Visibility>(label).unwrap()
    }

    #[test]
    fn a_label_shows_once_laid_out_and_while_shown() {
        let mut app = app();
        let id = view_with_label(&mut app, "A");
        app.update();
        app.update();
        let (label, view_camera) = label_in(&mut app, id);
        assert_eq!(
            visibility(&app, label),
            Visibility::Hidden,
            "not laid out yet"
        );

        // Stand in for the UI's layout and the render target's size, which
        // this app doesn't compute.
        app.world_mut().get_mut::<ComputedNode>(label).unwrap().size = Vec2::new(20.0, 10.0);
        let size = UVec2::new(64, 48);
        let projection = PerspectiveProjection {
            fov: 60f32.to_radians(),
            aspect_ratio: size.x as f32 / size.y as f32,
            ..default()
        };
        app.world_mut()
            .get_mut::<Camera>(view_camera)
            .unwrap()
            .computed = ComputedCameraValues {
            clip_from_view: projection.get_clip_from_view(),
            target_info: Some(RenderTargetInfo {
                physical_size: size,
                scale_factor: 1.0,
            }),
            ..default()
        };
        app.update();
        assert_eq!(visibility(&app, label), Visibility::Inherited);
        let node = app.world().get::<Node>(label).unwrap();
        // The origin, at the centre of a 64×48 view, less half the label.
        let (Val::Px(left), Val::Px(top)) = (node.left, node.top) else {
            panic!("not placed in pixels");
        };
        assert!(Vec2::new(left, top).distance(Vec2::new(22.0, 19.0)) < 1e-3);

        app.world_mut().get_mut::<WorldLabel>(label).unwrap().shown = false;
        app.update();
        assert_eq!(visibility(&app, label), Visibility::Hidden);
    }

    #[test]
    fn a_label_goes_with_its_anchor() {
        let mut app = app();
        let anchor = app.world_mut().spawn(Transform::default()).id();
        let label = app
            .world_mut()
            .spawn(label("tag".into(), 14.0, Color::NONE, anchor, Vec3::Y))
            .id();
        app.world_mut().entity_mut(anchor).despawn();
        assert!(app.world().get_entity(label).is_err());
    }
}
