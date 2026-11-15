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

/// A captured 8-bit frame as straight-alpha RGBA, as `ImageData` and PNG take
/// it. A frame drawn over a see-through clear colour holds premultiplied
/// colour, so its translucent edges would otherwise darken.
pub fn straight_rgba(image: &Image) -> Result<Vec<u8>, String> {
    let mut rgba = image.data.clone().ok_or("the image has no pixels")?;
    let format = image.texture_descriptor.format;
    let bgra = match format {
        TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb => false,
        TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb => true,
        _ => return Err(format!("can't read a {format:?} image as RGBA")),
    };
    let srgb = format.is_srgb();
    for px in rgba.as_chunks_mut::<4>().0 {
        if bgra {
            px.swap(0, 2);
        }
        unpremultiply(px, srgb);
    }
    Ok(rgba)
}

/// Divides colour by alpha in linear light, where blending premultiplied it.
fn unpremultiply(px: &mut [u8; 4], srgb: bool) {
    let a = px[3];
    if a == 0 || a == u8::MAX {
        return;
    }
    let alpha = f32::from(a) / 255.0;
    for c in &mut px[..3] {
        let v = f32::from(*c) / 255.0;
        let straight = if srgb {
            let linear = Srgba::gamma_function(v) / alpha;
            Srgba::gamma_function_inverse(linear.min(1.0))
        } else {
            (v / alpha).min(1.0)
        };
        *c = (straight * 255.0).round() as u8;
    }
}

#[cfg(test)]
mod tests {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension};

    use super::*;

    fn image(format: TextureFormat, pixels: &[[u8; 4]]) -> Image {
        Image::new(
            Extent3d {
                width: pixels.len() as u32,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            pixels.concat(),
            format,
            RenderAssetUsages::default(),
        )
    }

    #[test]
    fn opaque_and_clear_pixels_pass_through() {
        let pixels = [[10, 20, 30, 255], [0, 0, 0, 0]];
        let rgba = straight_rgba(&image(TextureFormat::Rgba8UnormSrgb, &pixels)).unwrap();
        assert_eq!(rgba, pixels.concat());
    }

    #[test]
    fn a_half_covered_edge_regains_its_full_colour() {
        // Pure red at half coverage, premultiplied: 128 linear, 188 sRGB-encoded.
        let linear = straight_rgba(&image(TextureFormat::Rgba8Unorm, &[[128, 0, 0, 128]])).unwrap();
        assert_eq!(linear, [255, 0, 0, 128]);
        let srgb =
            straight_rgba(&image(TextureFormat::Rgba8UnormSrgb, &[[188, 0, 0, 128]])).unwrap();
        assert!(srgb[0] >= 254, "{srgb:?}");
    }

    #[test]
    fn bgra_swaps_to_rgba() {
        let rgba = straight_rgba(&image(TextureFormat::Bgra8Unorm, &[[1, 2, 3, 255]])).unwrap();
        assert_eq!(rgba, [3, 2, 1, 255]);
    }

    #[test]
    fn a_float_image_is_refused() {
        let float = Image::new_fill(
            Extent3d::default(),
            TextureDimension::D2,
            &[0; 16],
            TextureFormat::Rgba32Float,
            RenderAssetUsages::default(),
        );
        assert!(straight_rgba(&float).is_err());
    }
}
