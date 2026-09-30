//! Bruit 3D raccordable (répétable sur les trois axes) pour les nuages, généré
//! au démarrage : texture 3D lue par assets/shaders/clouds.wgsl.
//! - R : bruit « Perlin-Worley » (formes arrondies en choux-fleurs des
//!   cumulus : Perlin creusé par des cellules de Worley) ;
//! - G : fBm de Worley plus fin (érosion des bords en boursouflures).
//! La forme générale des nuages reste donnée par la carte 2D (clouds.png) :
//! sans variation avec l'altitude, elle faisait des colonnes extrudées.
use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::math::Vec3;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// Côté de la texture (voxels).
const SIZE: usize = 64;

fn hash(x: i32, y: i32, z: i32, salt: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x8DA6_B343)
        ^ (y as u32).wrapping_mul(0xD816_3841)
        ^ (z as u32).wrapping_mul(0xCB1A_B31F)
        ^ salt.wrapping_mul(0x9E37_79B9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^ (h >> 15)
}

fn unit(h: u32) -> f32 {
    (h & 0xFFFF) as f32 / 65535.0
}

/// Worley (distance au point caractéristique le plus proche) inversé, sur une
/// grille de `cells` cellules par côté, raccordable. `p` dans [0, 1)³.
fn worley(p: Vec3, cells: i32, salt: u32) -> f32 {
    let q = p * cells as f32;
    let c = q.floor();
    let mut best = f32::MAX;
    for dz in -1..=1 {
        for dy in -1..=1 {
            for dx in -1..=1 {
                let n = c + Vec3::new(dx as f32, dy as f32, dz as f32);
                // Cellule ramenée dans la grille : raccord aux bords.
                let (wx, wy, wz) = (
                    (n.x as i32).rem_euclid(cells),
                    (n.y as i32).rem_euclid(cells),
                    (n.z as i32).rem_euclid(cells),
                );
                let feature = n + Vec3::new(unit(hash(wx, wy, wz, salt)), unit(hash(wx, wy, wz, salt + 1)), unit(hash(wx, wy, wz, salt + 2)));
                best = best.min((feature - q).length_squared());
            }
        }
    }
    1.0 - best.sqrt().min(1.0)
}

/// Bruit de gradient (Perlin) raccordable, période `period` cellules.
fn perlin(p: Vec3, period: i32, salt: u32) -> f32 {
    let q = p * period as f32;
    let c = q.floor();
    let f = q - c;
    let fade = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let grad = |dx: i32, dy: i32, dz: i32| -> f32 {
        let (x, y, z) = (
            (c.x as i32 + dx).rem_euclid(period),
            (c.y as i32 + dy).rem_euclid(period),
            (c.z as i32 + dz).rem_euclid(period),
        );
        let h = hash(x, y, z, salt);
        // Direction pseudo-aléatoire sur la sphère.
        let theta = unit(h) * std::f32::consts::TAU;
        let zc = unit(h >> 16) * 2.0 - 1.0;
        let r = (1.0 - zc * zc).sqrt();
        let g = Vec3::new(r * theta.cos(), r * theta.sin(), zc);
        g.dot(f - Vec3::new(dx as f32, dy as f32, dz as f32))
    };
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x00 = lerp(grad(0, 0, 0), grad(1, 0, 0), fade.x);
    let x10 = lerp(grad(0, 1, 0), grad(1, 1, 0), fade.x);
    let x01 = lerp(grad(0, 0, 1), grad(1, 0, 1), fade.x);
    let x11 = lerp(grad(0, 1, 1), grad(1, 1, 1), fade.x);
    lerp(lerp(x00, x10, fade.y), lerp(x01, x11, fade.y), fade.z)
}

fn remap(v: f32, lo: f32, hi: f32) -> f32 {
    ((v - lo) / (hi - lo)).clamp(0.0, 1.0)
}

/// Texture 3D RGBA8 (R : Perlin-Worley, G : Worley fin), répétée sur les
/// trois axes, filtrage linéaire.
pub fn cloud_noise_texture() -> Image {
    let mut data = vec![0u8; SIZE * SIZE * SIZE * 4];
    for z in 0..SIZE {
        for y in 0..SIZE {
            for x in 0..SIZE {
                let p = Vec3::new(x as f32, y as f32, z as f32) / SIZE as f32;
                let perlin_fbm = 0.5 + 0.5 * (perlin(p, 4, 10) + 0.5 * perlin(p, 8, 20) + 0.25 * perlin(p, 16, 30)) / 1.75 * 1.6;
                let worley_low = worley(p, 4, 100) * 0.625 + worley(p, 8, 200) * 0.25 + worley(p, 16, 300) * 0.125;
                // Perlin « creusé » par les cellules : amas ronds bien séparés.
                let shape = remap(perlin_fbm.clamp(0.0, 1.0), worley_low - 1.0, 1.0);
                let fine = worley(p, 8, 400) * 0.625 + worley(p, 16, 500) * 0.25 + worley(p, 32, 600) * 0.125;
                let i = ((z * SIZE + y) * SIZE + x) * 4;
                data[i] = (shape * 255.0) as u8;
                data[i + 1] = (fine * 255.0) as u8;
                data[i + 3] = 255;
            }
        }
    }
    let mut image = Image::new(
        Extent3d { width: SIZE as u32, height: SIZE as u32, depth_or_array_layers: SIZE as u32 },
        TextureDimension::D3,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..Default::default()
    });
    image
}
