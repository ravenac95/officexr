//! Port of `WaterVolumes.tsx`: each pool's water surface, a translucent
//! rippling plane at its surface height, foamy at the rim.

use bevy::light::NotShadowCaster;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use serde::Deserialize;

/// `motion/water.ts`'s `WaterVolume`: a pool's water, inside its rim.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WaterVolume {
    pub id: String,
    pub min_x: f32,
    pub max_x: f32,
    pub min_z: f32,
    pub max_z: f32,
    #[serde(default)]
    pub base_y: f32,
    pub surface_y: f32,
}

/// The water a view draws.
#[derive(Component, Default, Deref)]
pub struct ViewWater(pub Vec<WaterVolume>);

/// The water surfaces drawn for a view's [`ViewWater`].
#[derive(Component, Default)]
pub struct WaterIndex(Vec<Entity>);

impl WaterIndex {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The ripple, glint and rim foam of `WaterVolumes.tsx`'s shader, animated by the view's time.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct WaterMaterial {
    /// The surface's opacity.
    #[uniform(0)]
    opacity: f32,
}

impl Default for WaterMaterial {
    fn default() -> Self {
        Self { opacity: 0.78 }
    }
}

impl Material for WaterMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://officexr_render/water.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn enable_shadows() -> bool {
        false
    }

    // Seen from under the surface too, while swimming.
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// Redraws a view's water whenever its volumes change.
pub(crate) fn sync_water(
    mut commands: Commands,
    mut views: Query<(Entity, &ViewWater, Option<&mut WaterIndex>), Changed<ViewWater>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<WaterMaterial>>,
    mut material: Local<Option<Handle<WaterMaterial>>>,
) {
    for (view, water, index) in &mut views {
        let mut drawn = vec![];
        let material = material
            .get_or_insert_with(|| materials.add(WaterMaterial::default()))
            .clone();
        for volume in water.iter() {
            let size = Vec2::new(volume.max_x - volume.min_x, volume.max_z - volume.min_z);
            if size.x <= 0.0 || size.y <= 0.0 {
                continue;
            }
            let centre = Vec3::new(
                (volume.min_x + volume.max_x) / 2.0,
                volume.surface_y,
                (volume.min_z + volume.max_z) / 2.0,
            );
            let plane = Plane3d::new(Vec3::Y, size / 2.0).mesh().build();
            drawn.push(
                commands
                    .spawn((
                        Mesh3d(meshes.add(plane)),
                        MeshMaterial3d(material.clone()),
                        Transform::from_translation(centre),
                        NotShadowCaster,
                        ChildOf(view),
                    ))
                    .id(),
            );
        }
        match index {
            Some(mut index) => {
                for gone in std::mem::replace(&mut index.0, drawn) {
                    commands.entity(gone).try_despawn();
                }
            }
            None => {
                commands.entity(view).insert(WaterIndex(drawn));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{bare_app, camera, scene};

    fn pool(id: &str, x: f32) -> WaterVolume {
        serde_json::from_value(serde_json::json!({
            "id": id, "minX": x, "maxX": x + 4.0, "minZ": 0, "maxZ": 3, "baseY": 0, "surfaceY": 0.4
        }))
        .unwrap()
    }

    #[test]
    fn a_view_draws_one_surface_per_pool_at_its_surface_height() {
        let mut app = bare_app();
        app.insert_resource(scene(&[])).insert_resource(camera(0.0));
        app.update();
        let view = crate::views::find_view(app.world_mut(), 0).unwrap();
        app.world_mut()
            .entity_mut(view)
            .insert(ViewWater(vec![pool("a", 0.0), pool("b", 10.0)]));
        app.update();
        let mut surfaces = app
            .world_mut()
            .query_filtered::<&Transform, With<MeshMaterial3d<WaterMaterial>>>();
        let mut at: Vec<Vec3> = surfaces.iter(app.world()).map(|t| t.translation).collect();
        at.sort_by(|a, b| a.x.total_cmp(&b.x));
        assert_eq!(at, [Vec3::new(2.0, 0.4, 1.5), Vec3::new(12.0, 0.4, 1.5)]);

        app.world_mut()
            .entity_mut(view)
            .insert(ViewWater(vec![pool("a", 0.0)]));
        app.update();
        assert_eq!(surfaces.iter(app.world()).count(), 1);
    }
}
