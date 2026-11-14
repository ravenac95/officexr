use std::time::Duration;

use bevy::app::{PluginGroup, ScheduleRunnerPlugin};
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::window::ExitCondition;

use crate::{AssetBase, OfficeXrRenderPlugin};

/// The image the scene camera renders into.
#[derive(Resource, Clone)]
pub struct OffscreenTarget(pub Handle<Image>);

/// A windowless app that renders the scene camera into a `width`×`height`
/// [`OffscreenTarget`]. `default_plugins` is `DefaultPlugins`, less any
/// plugin that needs a display.
pub fn offscreen_app(
    base: AssetBase,
    default_plugins: impl PluginGroup,
    width: u32,
    height: u32,
) -> App {
    let mut app = App::new();
    app.add_plugins((
        base,
        default_plugins.set(WindowPlugin {
            primary_window: None,
            exit_condition: ExitCondition::DontExit,
            ..default()
        }),
        ScheduleRunnerPlugin::run_loop(Duration::ZERO),
        OfficeXrRenderPlugin,
    ));
    let image = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            width,
            height,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
    app.insert_resource(OffscreenTarget(image))
        .add_systems(Update, target_image);
    app
}

fn target_image(
    mut commands: Commands,
    cameras: Query<Entity, Added<Camera3d>>,
    target: Res<OffscreenTarget>,
) {
    for camera in &cameras {
        commands
            .entity(camera)
            .insert(RenderTarget::Image(target.0.clone().into()));
    }
}
