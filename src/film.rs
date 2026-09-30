//! Passe de post-traitement « pellicule » (assets/shaders/film.wgsl), après le
//! tonemapping : étalonnage que `ColorGrading` de Bevy ne sait pas faire
//! (ombres bleutées / hautes lumières dorées, verts moins criards, noirs
//! légèrement relevés) et grain de film animé.
//! Rendu calqué sur la FXAA de Bevy : un triangle plein écran qui lit l'image
//! et écrit dans l'autre texture principale de la vue.
use bevy::core_pipeline::schedule::{Core3d, Core3dSystems};
use bevy::core_pipeline::tonemapping::tonemapping;
use bevy::core_pipeline::FullscreenShader;
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer};
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::uniform::{ComponentUniforms, DynamicUniformIndex, UniformComponentPlugin};
use bevy::render::view::{ExtractedView, ViewTarget};
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
    /// x : relèvement des noirs (« fade » pellicule), yzw : inutilisés.
    pub params2: Vec4,
}

impl Default for FilmLook {
    fn default() -> Self {
        Self {
            shadows: Vec4::new(-0.4, 0.05, 0.6, 0.04),
            highlights: Vec4::new(0.7, 0.35, -0.5, 0.035),
            params: Vec4::new(0.03, 0.0, 0.92, 0.45),
            params2: Vec4::new(0.018, 0.0, 0.0, 0.0),
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
            .add_systems(Core3d, film.after(tonemapping).in_set(Core3dSystems::PostProcess));
    }
}

/// Grain animé : le temps change le tirage du bruit (24 fois par seconde,
/// dans le shader, comme une pellicule).
fn advance_grain(time: Res<Time>, mut looks: Query<&mut FilmLook>) {
    for mut look in &mut looks {
        look.params.y = time.elapsed_secs_wrapped();
    }
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
    view: ViewQuery<(&ViewTarget, &FilmPipelineId, &DynamicUniformIndex<FilmLook>)>,
    film_pipeline: Res<FilmPipeline>,
    pipeline_cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<FilmLook>>,
    mut ctx: RenderContext,
) {
    let (target, pipeline_id, uniform_index) = view.into_inner();
    let Some(pipeline) = pipeline_cache.get_render_pipeline(pipeline_id.0) else { return };
    let Some(uniform_binding) = uniforms.uniforms().binding() else { return };

    let post_process = target.post_process_write();
    let bind_group = ctx.render_device().create_bind_group(
        Some("film_bind_group"),
        &pipeline_cache.get_bind_group_layout(&film_pipeline.layout),
        &BindGroupEntries::sequential((post_process.source, &film_pipeline.sampler, uniform_binding)),
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
