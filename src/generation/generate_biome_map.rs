use std::process::exit;
use std::collections::HashSet;
use bevy::prelude::Resource;
use image::{ImageBuffer, Luma, Rgb, RgbImage};
use noise::{NoiseFn, Perlin, Fbm, OpenSimplex};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use crate::constants::{CHUNK_SIZE, SEA_LEVEL, WORLD_HEIGHT};
use crate::generation::biome::{ALL_BIOMES, BiomeType, get_biome_data};


fn zoom_2x(input: &Vec<Vec<bool>>) -> Vec<Vec<bool>> {
    let old_height = input.len();
    let old_width = if old_height > 0 { input[0].len() } else { 0 };

    let new_height = old_height * 2;
    let new_width = old_width * 2;

    let mut output = vec![vec![false; new_width]; new_height];

    for y in 0..old_height {
        for x in 0..old_width {
            let val = input[y][x];
            // Remplir un carré 2x2 dans la matrice agrandie
            output[y * 2][x * 2] = val;
            output[y * 2][x * 2 + 1] = val;
            output[y * 2 + 1][x * 2] = val;
            output[y * 2 + 1][x * 2 + 1] = val;
        }
    }

    output
}


fn apply_rule(input: &Vec<Vec<bool>>) -> Vec<Vec<bool>> {
    let height = input.len();
    let width = if height > 0 { input[0].len() } else { 0 };

    // Matrice résultat
    let mut output = input.clone();

    // Déplacements relatifs des 8 voisins
    let neighbors = [
        (-1, -1), (-1, 0), (-1, 1),
        (0, -1),          (0, 1),
        (1, -1),  (1, 0), (1, 1),
    ];

    for y in 0..height {
        for x in 0..width {
            let current = input[y][x];
            let mut true_count = 0;
            let mut false_count = 0;

            // Compter les voisins
            for (dy, dx) in &neighbors {
                let ny = y as isize + dy;
                let nx = x as isize + dx;

                if ny >= 0 && ny < height as isize && nx >= 0 && nx < width as isize {
                    if input[ny as usize][nx as usize] {
                        true_count += 1;
                    } else {
                        false_count += 1;
                    }
                }
            }

            // Appliquer la règle selon la majorité des voisins
            if !current && true_count > false_count {
                output[y][x] = true;
            } else if current && false_count > true_count {
                output[y][x] = false;
            }
            // Sinon la cellule reste identique
        }
    }

    output
}

fn convert_to_biomes(input: &Vec<Vec<bool>>) -> Vec<Vec<BiomeType>> {
    let height = input.len();
    let width = if height > 0 { input[0].len() } else { 0 };

    let mut result = vec![vec![BiomeType::Plain; width]; height];
    let mut rng = rand::thread_rng();

    let neighbors = [
        (-1, -1), (-1, 0), (-1, 1),
        (0, -1),          (0, 1),
        (1, -1),  (1, 0), (1, 1),
    ];

    for y in 0..height {
        for x in 0..width {
            let current = input[y][x];

            let mut true_count = 0;
            let mut false_count = 0;

            for (dy, dx) in &neighbors {
                let ny = y as isize + dy;
                let nx = x as isize + dx;

                if ny >= 0 && ny < height as isize && nx >= 0 && nx < width as isize {
                    if input[ny as usize][nx as usize] {
                        true_count += 1;
                    } else {
                        false_count += 1;
                    }
                } else {
                    // Bord hors grille = on peut choisir de l'ignorer ou le traiter comme false/true
                    // Ici on ignore
                }
            }

            let biome = match current {
                false => {
                    if true_count == 0 {
                        if rng.r#gen::<f64>() < 0.3 {
                            BiomeType::Abyss
                        }
                        else {
                            BiomeType::Ocean
                        }
                    } else {
                        BiomeType::Ocean
                    }
                }
                true => {
                    if false_count == 0 {
                        if rng.r#gen::<f64>() < 0.3 {
                            BiomeType::Mountain
                        }
                        else {
                            BiomeType::Plain
                        }
                    } else {
                        BiomeType::Plain
                    }
                }
            };

            result[y][x] = biome;
        }
    }

    result
}

fn smooth_biomes(input: &Vec<Vec<BiomeType>>) -> Vec<Vec<BiomeType>> {
    let height = input.len();
    let width = if height > 0 { input[0].len() } else { 0 };

    let mut output = input.clone();

    // Voisins orthogonaux (haut, bas, gauche, droite)
    let neighbors = [
        (-1, 0),
        (1, 0),
        (0, -1),
        (0, 1),
    ];

    for y in 0..height {
        for x in 0..width {
            match input[y][x] {
                BiomeType::Abyss => {
                    // Vérifie si tous voisins orthogonaux sont Ocean (ou hors limites)
                    let all_ocean = neighbors.iter().all(|(dy, dx)| {
                        let ny = y as isize + dy;
                        let nx = x as isize + dx;
                        if ny >= 0 && ny < height as isize && nx >= 0 && nx < width as isize {
                            matches!(input[ny as usize][nx as usize], BiomeType::Ocean)
                        } else {
                            true // bord traité comme ok
                        }
                    });

                    if all_ocean {
                        output[y][x] = BiomeType::Ocean;
                    }
                }

                BiomeType::Mountain => {
                    // Vérifie si tous voisins orthogonaux sont Plain (ou hors limites)
                    let all_plain = neighbors.iter().all(|(dy, dx)| {
                        let ny = y as isize + dy;
                        let nx = x as isize + dx;
                        if ny >= 0 && ny < height as isize && nx >= 0 && nx < width as isize {
                            matches!(input[ny as usize][nx as usize], BiomeType::Plain)
                        } else {
                            true // bord traité comme ok
                        }
                    });

                    if all_plain {
                        output[y][x] = BiomeType::Plain;
                    }
                }

                _ => {} // autres biomes inchangés
            }
        }
    }

    output
}


#[derive(Resource, Default, Clone)]
pub struct BiomeMap {
    biomes_map: Vec<Vec<BiomeType>>
}

impl BiomeMap {

    pub fn new() -> Self {
        Self {
            biomes_map: vec![vec![BiomeType::Plain; 1600]; 1600],
        }
    }

    pub fn generate(&mut self) {
        //une case = 4096 * 4096 block
        let mut biome_map_size_0 = vec![vec![true; 100]; 100];
        let noise = OpenSimplex::new(0);
        let mut rng = rand::thread_rng();
        for x in 0..100 {
            for z in 0..100 {
                // Génère un float entre 0 et 1
                let r: f64 = rng.r#gen();
                if r < 0.5 {
                    biome_map_size_0[x][z] = false;
                }
            }
        }

        let biome_map_size_1 = zoom_2x(&biome_map_size_0);
        let biome_map_size_1 = apply_rule(&biome_map_size_1);
        let biome_map_size_2 = zoom_2x(&biome_map_size_1);
        let biome_map_size_3 = zoom_2x(&biome_map_size_2);
        let biome_map_size_4 = zoom_2x(&biome_map_size_3);
        let biome_map_size_4 = apply_rule(&biome_map_size_4);


        //generate img
        let mut img: RgbImage = ImageBuffer::new(1600 as u32, 1600 as u32);

        for (y, row) in biome_map_size_4.iter().enumerate() {
            for (x, &value) in row.iter().enumerate() {
                let color = if value {
                    Rgb([0, 255, 0])
                } else {
                    Rgb([0, 0, 255])
                };
                img.put_pixel(x as u32, y as u32, color);
            }
        }

        img.save("biome_map_size_4.png").expect("Erreur lors de la sauvegarde de l'image");

        let biomes = convert_to_biomes(&biome_map_size_4);
        self.biomes_map = smooth_biomes(&biomes);


        let mut img_biomes: RgbImage = ImageBuffer::new(1600 as u32, 1600 as u32);

        for (y, row) in self.biomes_map.iter().enumerate() {
            for (x, &value) in row.iter().enumerate() {
                let color =  if value == BiomeType::Ocean {
                    Rgb([173, 216, 230])
                } else if value == BiomeType::Abyss {
                    Rgb([25, 25, 112])
                } else if value == BiomeType::Plain {
                    Rgb([0, 255, 0])
                } else if value == BiomeType::Mountain {
                    Rgb([125, 125, 125])
                }else {
                    Rgb([255, 0, 0])
                };

                img_biomes.put_pixel(x as u32, y as u32, color);
            }
        }
        img_biomes.save("biome_map.png").expect("Erreur lors de la sauvegarde de l'image");
    }


    pub fn get_biome(&self, x_block: i64, z_block: i64) -> BiomeType {
        let biome_size = 256;
        let map_size = self.biomes_map.len() as i32; // ici 1600

        // calcul position case relative à 0,0
        let x_index = (x_block.div_euclid(biome_size)) + (map_size / 2) as i64;
        let z_index = (z_block.div_euclid(biome_size)) + (map_size / 2) as i64;


        self.biomes_map[z_index as usize][x_index as usize]
    }
}
