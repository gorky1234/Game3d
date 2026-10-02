//! Brume au ras du sol (assets/shaders/height_fog.wgsl) : sa densité décroît
//! avec l'altitude, elle s'accumule dans les vallées et laisse les sommets
//! nets au-dessus. La brume de distance de Bevy (`DistanceFog`), de même
//! densité partout, ne donnait qu'un voile uniforme : pas ces plans successifs
//! (crête sombre, vallée laiteuse, crête suivante plus pâle) des paysages de
//! montagne.
//!
//! Passe plein écran juste après le rendu de la scène : reconstruit la position de chaque
//! pixel depuis la profondeur, intègre la densité le long du rayon (formule
//! exacte pour une densité exponentielle en altitude) et mélange vers la
//! couleur de la brume de distance (mise à jour avec l'heure et la météo, voir
//! `daylight_cycle`), avec le même halo autour du soleil. Le ciel reçoit la
//! brume d'un rayon de `FogParams::sky_distance` blocs : bande de brume à
//! l'horizon, raccordée au relief lointain.
use bevy::core_pipeline::schedule::{Core3d, Core3dSystems};
use bevy::anti_alias::taa::temporal_anti_alias;
use bevy::core_pipeline::FullscreenShader;
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, texture_depth_2d, uniform_buffer};
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::texture::{FallbackImage, GpuImage};
use bevy::render::renderer::RenderDevice;
use crate::render::cloud_shadows::SunThroughClouds;
use crate::render::far_terrain::{FarShadowImage, FarTreeShadowImage};
use crate::render::skybox::{BiomeAir, SkyState, Sun};
use crate::texture::{PlantMaterial, TextureAtlasMaterial};
use crate::world::block::BlockType;
use crate::world::load_save_chunk::WorldData;
use crate::world::weather::Weather;
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderContext, ViewQuery};
use bevy::render::uniform::{ComponentUniforms, DynamicUniformIndex, UniformComponentPlugin};
use bevy::render::view::{ExtractedView, ViewDepthTexture, ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms};
use bevy::render::{GpuResourceAppExt, Render, RenderApp, RenderStartup, RenderSystems};

/// Réglages, sur la caméra (mis à jour par `daylight_cycle`, skybox.rs).
#[derive(Component, Clone, Copy, ExtractComponent, ShaderType)]
pub struct HeightFog {
    /// rgb : couleur (linéaire) de la brume, a : inutilisé.
    pub color: Vec4,
    /// rgb : couleur × intensité du halo vers le soleil, a : exposant du halo.
    pub sun_color: Vec4,
    /// xyz : direction vers le soleil.
    pub sun_direction: Vec4,
    /// x : densité (par bloc) à l'altitude de base, y : altitude de base, z :
    /// hauteur (blocs) sur laquelle la densité est divisée par e, w :
    /// distance prêtée au ciel.
    pub params: Vec4,
    /// Brume ombrée : ombres du relief (voir far_shadows.rs), textures fine
    /// et grossière (coin, 1 / largeur, prête) et fondu dans le temps.
    pub far_area: Vec4,
    pub tree_area: Vec4,
    pub far_timing: Vec4,
    /// Rayons de soleil : x intensité (0 : aucun), y part du soleil qui
    /// passe les nuages, z temps (s, poussière dans la lumière).
    pub rays: Vec4,
    /// Sous l'eau : x profondeur de la caméra sous la surface (0 : hors de
    /// l'eau), yzw couleur de l'eau.
    pub water: Vec4,
    /// Nappe de brume des marais au ras de l'eau : x densité, y altitude de
    /// sa base (eau ou sol sous la caméra), z hauteur d'atténuation.
    pub marsh: Vec4,
}

impl Default for HeightFog {
    fn default() -> Self {
        Self {
            color: Vec4::new(0.7, 0.78, 0.86, 1.0),
            sun_color: Vec4::ZERO,
            sun_direction: Vec4::Y,
            params: Vec4::new(0.0, 0.0, 40.0, 12_000.0),
            far_area: Vec4::ZERO,
            tree_area: Vec4::ZERO,
            far_timing: Vec4::ZERO,
            rays: Vec4::ZERO,
            water: Vec4::ZERO,
            marsh: Vec4::new(0.0, 0.0, 2.5, 0.0),
        }
    }
}

pub struct HeightFogPlugin;

impl Plugin for HeightFogPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((ExtractComponentPlugin::<HeightFog>::default(), UniformComponentPlugin::<HeightFog>::default()))
            .add_plugins(ExtractResourcePlugin::<FogShadowImages>::default())
            .add_systems(Update, (share_shadow_images, update_fog_extras));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .init_gpu_resource::<SpecializedRenderPipelines<HeightFogPipeline>>()
            .add_systems(RenderStartup, init_height_fog_pipeline)
            .add_systems(Render, prepare_height_fog_pipelines.in_set(RenderSystems::Prepare))
            // Avant le TAA, qui lissera la brume avec le reste de l'image : la
            // profondeur lue ici est celle de l'image décalée (jitter) du TAA,
            // une brume appliquée après lui faisait scintiller les contours
            // sur le ciel.
            .add_systems(Core3d, height_fog.before(temporal_anti_alias).in_set(Core3dSystems::EarlyPostProcess));
    }
}

/// Textures des ombres du relief lues par la passe (brume ombrée).
#[derive(Resource, Clone, ExtractResource)]
struct FogShadowImages {
    near: Handle<Image>,
    tree: Handle<Image>,
}

fn share_shadow_images(
    mut commands: Commands,
    near: Option<Res<FarShadowImage>>,
    tree: Option<Res<FarTreeShadowImage>>,
    existing: Option<Res<FogShadowImages>>,
) {
    if let (Some(near), Some(tree), None) = (near, tree, existing) {
        commands.insert_resource(FogShadowImages { near: near.0.clone(), tree: tree.0.clone() });
    }
}

/// Réglages venus d'ailleurs : ombres du relief (copiées du matériau du
/// feuillage, tenu à jour par far_terrain.rs), rayons de soleil, et eau
/// autour de la caméra.
#[allow(clippy::too_many_arguments)]
fn update_fog_extras(
    mut fogs: Query<(&mut HeightFog, &GlobalTransform)>,
    atlas: Option<Res<TextureAtlasMaterial>>,
    plants: Res<Assets<PlantMaterial>>,
    clouds: Res<SunThroughClouds>,
    suns: Query<(&Transform, &DirectionalLight), With<Sun>>,
    sky: Res<SkyState>,
    weather: Res<Weather>,
    world: Res<WorldData>,
    time: Res<Time>,
    air: Res<BiomeAir>,
) {
    let far = atlas.as_ref().and_then(|a| plants.get(&a.foliage_handle)).map(|m| (m.extension.far_area, m.extension.far_tree_area, m.extension.far_timing));
    let sun = suns.single().ok();
    for (mut fog, camera) in &mut fogs {
        if let Some((area, tree, timing)) = far {
            fog.far_area = area;
            fog.tree_area = tree;
            fog.far_timing = timing;
        }
        // Rayons : surtout soleil bas (lumière rasante entre les arbres et
        // les crêtes), jamais la nuit ni par temps couvert.
        if let Some((transform, _)) = sun {
            let elevation = transform.back().y.max(0.0);
            let low = 1.0 - (elevation / 0.7).clamp(0.0, 1.0);
            fog.rays = Vec4::new(sky.daylight * weather.current.sun * (0.35 + 0.65 * low), clouds.0, time.elapsed_secs_wrapped(), 0.0);
        }
        // Sous l'eau : profondeur de la caméra sous la surface.
        let p = camera.translation();
        let (x, y, z) = (p.x.floor() as isize, p.y.floor() as isize, p.z.floor() as isize);
        let mut depth = 0.0;
        if world.get_block_at(x, y, z) == BlockType::Water {
            let mut top = y;
            while top < y + 64 && world.get_block_at(x, top + 1, z) == BlockType::Water {
                top += 1;
            }
            depth = (top as f32 + 0.9 - p.y).max(0.05);
        }
        let lit = 0.15 + 0.85 * sky.daylight;
        fog.water = Vec4::new(depth, 0.03 * lit, 0.14 * lit, 0.16 * lit);
        // Nappe de brume des marais : sa base suit l'eau ou le sol sous la
        // caméra (lissée : pas de saut d'un bloc à l'autre), plus épaisse au
        // petit matin et au crépuscule.
        let swamp = (air.swamp + air.gloom * 0.5).min(1.0);
        if swamp > 0.01 {
            let mut ground = y;
            while ground > y - 48 && !matches!(world.get_block_at(x, ground, z), BlockType::Water) && !world.get_block_at(x, ground, z).is_solid() {
                ground -= 1;
            }
            let base = ground as f32 + 1.0;
            let current = if fog.marsh.y == 0.0 { base } else { fog.marsh.y };
            fog.marsh.y = current + (base - current) * (time.delta_secs() * 0.8).min(1.0);
        }
        let dusk = 1.0 - (sky.daylight * 2.0 - 1.0).abs().min(1.0);
        let on = !crate::debug_capture::is_disabled("brume_marais");
        fog.marsh.x = if on { swamp * (0.012 + 0.03 * sky.mist() + 0.015 * dusk) * weather.current.fog.max(0.6) } else { 0.0 };
        fog.marsh.z = 1.6 + 1.0 * sky.mist();
    }
}

#[derive(Resource)]
struct HeightFogPipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    fullscreen_shader: FullscreenShader,
    fragment_shader: Handle<Shader>,
}

fn init_height_fog_pipeline(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    fullscreen_shader: Res<FullscreenShader>,
    asset_server: Res<AssetServer>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "height_fog_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_depth_2d(),
                uniform_buffer::<ViewUniform>(true),
                uniform_buffer::<HeightFog>(true),
                texture_2d(TextureSampleType::Float { filterable: true }),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    );
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });
    commands.insert_resource(HeightFogPipeline {
        layout,
        sampler,
        fullscreen_shader: fullscreen_shader.clone(),
        fragment_shader: asset_server.load("shaders/height_fog.wgsl"),
    });
}

impl SpecializedRenderPipeline for HeightFogPipeline {
    type Key = TextureFormat;

    fn specialize(&self, format: Self::Key) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("height_fog".into()),
            layout: vec![self.layout.clone()],
            vertex: self.fullscreen_shader.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: self.fragment_shader.clone(),
                // Mélange prémultiplié sur l'image : couleur de la brume ×
                // opacité (1 - transmittance), plus les rayons de soleil.
                targets: vec![Some(ColorTargetState {
                    format,
                    blend: Some(BlendState {
                        color: BlendComponent {
                            src_factor: BlendFactor::One,
                            dst_factor: BlendFactor::OneMinusSrcAlpha,
                            operation: BlendOperation::Add,
                        },
                        alpha: BlendComponent::OVER,
                    }),
                    write_mask: ColorWrites::COLOR,
                })],
                ..default()
            }),
            ..default()
        }
    }
}

#[derive(Component)]
struct HeightFogPipelineId(CachedRenderPipelineId);

fn prepare_height_fog_pipelines(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<HeightFogPipeline>>,
    fog_pipeline: Res<HeightFogPipeline>,
    cameras: Query<(Entity, &ExtractedView), (With<ExtractedCamera>, With<HeightFog>)>,
) {
    for (entity, view) in &cameras {
        let id = pipelines.specialize(&pipeline_cache, &fog_pipeline, view.target_format);
        commands.entity(entity).insert(HeightFogPipelineId(id));
    }
}

fn height_fog(
    view: ViewQuery<(&ViewTarget, &ViewDepthTexture, &HeightFogPipelineId, &ViewUniformOffset, &DynamicUniformIndex<HeightFog>)>,
    fog_pipeline: Res<HeightFogPipeline>,
    pipeline_cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<HeightFog>>,
    view_uniforms: Res<ViewUniforms>,
    images: Option<Res<FogShadowImages>>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    fallback: Res<FallbackImage>,
    mut ctx: RenderContext,
) {
    let (target, depth, pipeline_id, view_offset, fog_index) = view.into_inner();
    let Some(pipeline) = pipeline_cache.get_render_pipeline(pipeline_id.0) else { return };
    let (Some(fog_binding), Some(view_binding)) = (uniforms.uniforms().binding(), view_uniforms.uniforms.binding()) else { return };
    let texture = |handle: Option<&Handle<Image>>| handle.and_then(|h| gpu_images.get(h)).map_or(&fallback.d2.texture_view, |g| &g.texture_view);
    let near = texture(images.as_ref().map(|i| &i.near));
    let tree = texture(images.as_ref().map(|i| &i.tree));

    let bind_group = ctx.render_device().create_bind_group(
        Some("height_fog_bind_group"),
        &pipeline_cache.get_bind_group_layout(&fog_pipeline.layout),
        &BindGroupEntries::sequential((depth.view(), view_binding, fog_binding, near, tree, &fog_pipeline.sampler)),
    );
    // Dessinée par-dessus l'image en place (comme le ciel de Bevy), sans
    // échange des deux textures principales de la vue.
    let pass_descriptor = RenderPassDescriptor {
        label: Some("height_fog"),
        color_attachments: &[Some(target.get_color_attachment())],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    };
    let mut render_pass = ctx.command_encoder().begin_render_pass(&pass_descriptor);
    render_pass.set_pipeline(pipeline);
    render_pass.set_bind_group(0, &bind_group, &[view_offset.offset, fog_index.index()]);
    render_pass.draw(0..3, 0..1);
}
