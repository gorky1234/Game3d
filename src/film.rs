//! Passe de post-traitement « pellicule » (assets/shaders/film.wgsl), après le
//! tonemapping : étalonnage que `ColorGrading` de Bevy ne sait pas faire
//! (ombres bleutées / hautes lumières dorées, verts moins criards, noirs
//! légèrement relevés) et grain de film animé.
//! Rendu calqué sur la FXAA de Bevy : un triangle plein écran qui lit l'image
//! et écrit dans l'autre texture principale de la vue.
use bevy::core_pipeline::schedule::{Core3d, Core3dSystems};
use bevy::anti_alias::contrast_adaptive_sharpening::cas;
use bevy::core_pipeline::tonemapping::tonemapping;
use bevy::core_pipeline::FullscreenShader;
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, texture_depth_2d, uniform_buffer};
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::uniform::{ComponentUniforms, DynamicUniformIndex, UniformComponentPlugin};
use bevy::render::view::{ExtractedView, ViewDepthTexture, ViewTarget};
use bevy::render::{GpuResourceAppExt, Render, RenderApp, RenderStartup, RenderSystems};

/// Réglages de la passe, sur la caméra. Couleurs en espace gamma (ajoutées à
/// l'image affichée).
#[derive(Component, Clone, Copy, ExtractComponent, ShaderType)]
pub struct FilmLook {
    /// rgb : teinte ajoutée dans les ombres, a : force.
    pub shadows: Vec4,
    /// rgb : teinte ajoutée dans les hautes lumières, a : force.
    pub highlights: Vec4,
    /// x : force du grain, y : temps (s, mis à jour chaque image), z :
    /// saturation globale, w : désaturation des verts francs (0..1).
    pub params: Vec4,
    /// x : relèvement des noirs (« fade » pellicule), y : distorsion de
    /// chaleur (0..1), z : position de l'horizon à l'écran (v, 0 en haut),
    /// w : plan proche de la caméra (distance depuis la profondeur).
    pub params2: Vec4,
    /// Reflets de lentille : xy position du soleil à l'écran (uv), z force
    /// (0 : soleil derrière la caméra, nuit, ciel couvert), w : flou
    /// atmosphérique lointain (0..1).
    pub sun: Vec4,
}

impl Default for FilmLook {
    fn default() -> Self {
        Self {
            shadows: Vec4::new(-0.4, 0.05, 0.6, 0.04),
            highlights: Vec4::new(0.7, 0.35, -0.5, 0.035),
            params: Vec4::new(0.03, 0.0, 0.92, 0.45),
            params2: Vec4::new(0.018, 0.0, 0.0, 0.1),
            sun: Vec4::ZERO,
        }
    }
}

pub struct FilmPlugin;

impl Plugin for FilmPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((ExtractComponentPlugin::<FilmLook>::default(), UniformComponentPlugin::<FilmLook>::default()))
            .add_systems(Update, advance_grain);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .init_gpu_resource::<SpecializedRenderPipelines<FilmPipeline>>()
            .add_systems(RenderStartup, init_film_pipeline)
            .add_systems(Render, prepare_film_pipelines.in_set(RenderSystems::Prepare))
            // Après l'accentuation (CAS), elle aussi après le tonemapping :
            // sans ordre entre les deux, Bevy pouvait les exécuter dans un
            // ordre et envoyer leurs commandes au GPU dans l'autre -- chacune
            // lisait alors la mauvaise texture de la paire, et l'écran
            // montrait l'image d'avant le tonemapping (claire, saturée), au
            // gré du moindre changement dans l'ordre d'exécution.
            .add_systems(Core3d, film.after(tonemapping).after(cas).in_set(Core3dSystems::PostProcess));
    }
}

/// Grain animé : le temps change le tirage du bruit (24 fois par seconde,
/// dans le shader, comme une pellicule).
fn advance_grain(
    time: Res<Time>,
    mut looks: Query<(&mut FilmLook, &GlobalTransform, &Projection, &Camera)>,
    suns: Query<&Transform, With<crate::render::skybox::Sun>>,
    air: Res<crate::render::skybox::BiomeAir>,
    sky: Res<crate::render::skybox::SkyState>,
    weather: Res<crate::world::weather::Weather>,
    clouds: Res<crate::render::cloud_shadows::SunThroughClouds>,
    quality: Res<crate::graphics_quality::GraphicsQuality>,
) {
    let to_sun = suns.single().ok().map(|t| t.back().as_vec3());
    for (mut look, transform, projection, camera) in &mut looks {
        look.params.y = time.elapsed_secs_wrapped();
        // Distorsion de chaleur : régions sèches, en plein jour, soleil
        // haut (sol brûlant), pas sous les nuages.
        let heat = air.dry * sky.daylight * weather.current.sun * smoothstep_f32(0.25, 0.7, sky.sun_height());
        look.params2.y = heat;
        // Horizon à l'écran : là où ondule l'air au-dessus du sol lointain.
        if let Projection::Perspective(p) = projection {
            let forward = transform.forward();
            let pitch = forward.y.clamp(-0.99, 0.99).asin();
            let ndc_y = (-pitch).tan() / (p.fov * 0.5).tan();
            look.params2.z = 0.5 - ndc_y * 0.5;
            look.params2.w = p.near;
        }
        // Soleil à l'écran pour les reflets de lentille (le shader vérifie
        // qu'il n'est pas masqué par le décor).
        look.sun.z = 0.0;
        if let Some(dir) = to_sun.filter(|d| d.y > -0.02 && sky.daylight > 0.05) {
            if let Some(ndc) = camera.world_to_ndc(transform, transform.translation() + dir * 10_000.0) {
                if ndc.z > 0.0 && ndc.x.abs() < 1.6 && ndc.y.abs() < 1.6 {
                    look.sun.x = ndc.x * 0.5 + 0.5;
                    look.sun.y = 0.5 - ndc.y * 0.5;
                    look.sun.z = sky.daylight * weather.current.sun * clouds.0;
                }
            }
        }
        // Flou lointain : léger, qualité haute seulement (la profondeur de
        // champ de Bevy s'arrête à 2000 blocs).
        look.sun.w = if *quality == crate::graphics_quality::GraphicsQuality::High { 1.0 } else { 0.0 };
    }
}

fn smoothstep_f32(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[derive(Resource)]
struct FilmPipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    fullscreen_shader: FullscreenShader,
    fragment_shader: Handle<Shader>,
}

fn init_film_pipeline(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    fullscreen_shader: Res<FullscreenShader>,
    asset_server: Res<AssetServer>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "film_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<FilmLook>(true),
                texture_depth_2d(),
            ),
        ),
    );
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });
    commands.insert_resource(FilmPipeline {
        layout,
        sampler,
        fullscreen_shader: fullscreen_shader.clone(),
        fragment_shader: asset_server.load("shaders/film.wgsl"),
    });
}

impl SpecializedRenderPipeline for FilmPipeline {
    type Key = TextureFormat;

    fn specialize(&self, format: Self::Key) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("film".into()),
            layout: vec![self.layout.clone()],
            vertex: self.fullscreen_shader.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: self.fragment_shader.clone(),
                targets: vec![Some(ColorTargetState { format, blend: None, write_mask: ColorWrites::ALL })],
                ..default()
            }),
            ..default()
        }
    }
}

#[derive(Component)]
struct FilmPipelineId(CachedRenderPipelineId);

fn prepare_film_pipelines(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<FilmPipeline>>,
    film_pipeline: Res<FilmPipeline>,
    cameras: Query<(Entity, &ExtractedView), (With<ExtractedCamera>, With<FilmLook>)>,
) {
    for (entity, view) in &cameras {
        let id = pipelines.specialize(&pipeline_cache, &film_pipeline, view.target_format);
        commands.entity(entity).insert(FilmPipelineId(id));
    }
}

fn film(
    view: ViewQuery<(&ViewTarget, &FilmPipelineId, &DynamicUniformIndex<FilmLook>, &ViewDepthTexture)>,
    film_pipeline: Res<FilmPipeline>,
    pipeline_cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<FilmLook>>,
    mut ctx: RenderContext,
) {
    let (target, pipeline_id, uniform_index, depth) = view.into_inner();
    let Some(pipeline) = pipeline_cache.get_render_pipeline(pipeline_id.0) else { return };
    let Some(uniform_binding) = uniforms.uniforms().binding() else { return };

    let post_process = target.post_process_write();
    let bind_group = ctx.render_device().create_bind_group(
        Some("film_bind_group"),
        &pipeline_cache.get_bind_group_layout(&film_pipeline.layout),
        &BindGroupEntries::sequential((post_process.source, &film_pipeline.sampler, uniform_binding, depth.view())),
    );
    let pass_descriptor = RenderPassDescriptor {
        label: Some("film"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: post_process.destination,
            depth_slice: None,
            resolve_target: None,
            ops: Operations::default(),
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    };
    let mut render_pass = ctx.command_encoder().begin_render_pass(&pass_descriptor);
    render_pass.set_pipeline(pipeline);
    render_pass.set_bind_group(0, &bind_group, &[uniform_index.index()]);
    render_pass.draw(0..3, 0..1);
}
