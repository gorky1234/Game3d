"""Tuiles du terrain lisse en photo haute résolution (Poly Haven, CC0, sources
2k dans tools/polyhaven/, réduites à TILE = 1024) avec leurs cartes de
normales et de rugosité :
- grass.png : forrest_ground_01 (sol herbeux) ;
- dirt.png : brown_mud_dry (terre sèche, graviers) ;
- rock.png : rock_05 ;
- sand.png : sand_01 ;
- snow.png : snow_02 (brindilles sombres atténuées).

Couleur recalée sur la tuile qu'elle remplace (moyenne par canal, contraste
en partie) : la palette réglée du jeu est conservée, seuls le grain et la
netteté changent. À lancer AVANT gen_badlands_textures.py (terre rouge tirée
de sand.png) et gen_rock_macro.py (couleur recalée sur rock.png) ; relancer
ce script repartirait des tuiles déjà remplacées (recalage inchangé à peu de
chose près).

Sources : https://polyhaven.com/a/<nom>, fichiers <nom>_{diff,nor_gl,rough}_2k.png
Usage : python3 tools/gen_terrain_photos.py
"""
import json
import numpy as np
from PIL import Image

Image.MAX_IMAGE_PIXELS = None
SRC = "tools/polyhaven/"
ASSETS = "assets/"
COLOR, NORMAL, ROUGH = (ASSETS + f for f in ("atlas_texture.png", "atlas_texture_normal.png", "atlas_texture_metallic_roughness.png"))
JSON = ASSETS + "atlas_texture.json"
TILE, MARGIN = 1024, 32

# (tuile, source, part du contraste d'origine de la tuile gardée : 0 =
# contraste de la photo, 1 = celui de l'ancienne tuile)
TILES = [
    ("grass.png", "forrest_ground_01", 0.5),
    ("dirt.png", "brown_mud_dry", 0.3),
    ("rock.png", "rock_05", 0.3),
    ("sand.png", "sand_01", 0.3),
    ("snow.png", "snow_02", 0.5),
]

color = Image.open(COLOR).convert("RGBA")
normal = Image.open(NORMAL).convert("RGBA")
rough = Image.open(ROUGH).convert("RGBA")
meta = json.load(open(JSON))
assert meta["frames"]["grass.png"]["frame"]["w"] == TILE, "atlas pas encore en 1024 (voir upscale_atlas_2x.py)"


def load(path):
    """Image en flottants 0..255, 3 canaux, TILE x TILE (PNG 8 ou 16 bits)."""
    im = Image.open(path)
    a = np.array(im).astype(np.float32)
    if a.max() > 255:
        a = a / 257.0
    if a.ndim == 2:
        a = np.stack([a] * 3, -1)
    elif a.shape[-1] == 2:  # gris + alpha
        a = np.stack([a[..., 0]] * 3, -1)
    im = Image.fromarray(np.clip(a[..., :3], 0, 255).astype(np.uint8), "RGB")
    return np.array(im.resize((TILE, TILE), Image.Resampling.LANCZOS)).astype(np.float32)


def crop(atlas, name):
    f = meta["frames"][name]["frame"]
    x, y = int(f["x"]), int(f["y"])
    return np.array(atlas.crop((x, y, x + TILE, y + TILE))).astype(np.float32), x, y


def put(atlas, a, x, y):
    """Tuile répétable : marge = contenu enroulé."""
    a = np.clip(a, 0, 255).astype(np.uint8)
    padded = np.pad(a, ((MARGIN, MARGIN), (MARGIN, MARGIN), (0, 0)), mode="wrap")
    atlas.paste(Image.fromarray(padded, "RGBA"), (x - MARGIN, y - MARGIN))


def luma(rgb):
    return rgb @ np.array([0.2126, 0.7152, 0.0722], np.float32)


opaque = lambda a: np.dstack([a, np.full(a.shape[:2], 255.0)])

for name, src, keep_contrast in TILES:
    old, x, y = crop(color, name)
    old = old[..., :3]
    c = load(f"{SRC}{src}_diff_2k.png")
    if src == "snow_02":
        # Brindilles et traces sombres : ramenées vers le blanc de la neige
        # (une tuile répétée tous les 4 blocs les alignait en motif).
        l = luma(c)
        floor = np.percentile(l, 25)
        dark = np.clip((floor - l) / max(floor, 1.0), 0.0, 1.0)[..., None]
        c = c + (c.mean((0, 1)) - c) * np.clip(dark * 3.0, 0.0, 1.0)
    # Recalage : moyenne par canal de l'ancienne tuile, écart-type en
    # partie seulement (grain de la photo gardé).
    mean_c, std_c = c.reshape(-1, 3).mean(0), c.reshape(-1, 3).std(0)
    mean_o, std_o = old.reshape(-1, 3).mean(0), old.reshape(-1, 3).std(0)
    std_t = std_c * (1 - keep_contrast) + std_o * keep_contrast
    c = (c - mean_c) / np.maximum(std_c, 1e-3) * std_t + mean_o
    n = load(f"{SRC}{src}_nor_gl_2k.png")
    r = load(f"{SRC}{src}_rough_2k.png")[..., 0]
    # Canal G : rugosité, B : métal (0), comme le reste de l'atlas.
    mr = np.stack([np.full_like(r, 255), r, np.zeros_like(r), np.full_like(r, 255)], -1)
    put(color, opaque(c), x, y)
    put(normal, opaque(n), x, y)
    put(rough, mr, x, y)
    print(name, "<-", src, "moyenne", mean_o.round(1))

color.save(COLOR)
normal.save(NORMAL)
rough.save(ROUGH)
