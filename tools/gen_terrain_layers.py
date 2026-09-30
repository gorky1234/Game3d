"""Couches du terrain lisse (voir src/texture.rs et assets/shaders/terrain.wgsl).

1. Tuile de croûte de sel (salt.png : couleur, normales, rugosité) : polygones
   de sel aux bords relevés (déserts de sel, gypse), dans une nouvelle rangée
   de l'atlas si elle n'y est pas encore.
2. Cartes de hauteur des tuiles du terrain, dans le canal R (inutilisé par
   Bevy) de l'atlas de rugosité : le shader mélange les couches selon ce
   relief (le sable remplit les creux entre les pavés, l'herbe pousse entre
   les cailloux) au lieu d'un simple fondu. Hauteur intégrée depuis la carte
   de normales (tuiles répétables : intégration exacte par FFT), mêlée à la
   luminance (sommets clairs, creux sombres).

Usage : python3 tools/gen_terrain_layers.py (relançable).
"""
import json

import numpy as np
from PIL import Image

Image.MAX_IMAGE_PIXELS = None
ASSETS = "assets/"
JSON = ASSETS + "atlas_texture.json"
FILES = ("atlas_texture.png", "atlas_texture_normal.png", "atlas_texture_metallic_roughness.png")
TILE, MARGIN = 1024, 32
SLOT = TILE + 2 * MARGIN
# Côtés multiples de 512 (voir tools/pad_atlas.py).
PAD = 512

# Tuiles dont le terrain lisse lit la hauteur (voir `layers`, src/texture.rs).
HEIGHT_TILES = [
    "grass.png", "dirt.png", "rock.png", "sand.png", "snow.png", "red_sand.png", "red_rock.png",
    "litter.png", "podzol.png", "mud.png", "gravel.png", "sandstone.png", "salt.png",
]

meta = json.load(open(JSON))
frames = meta["frames"]
atlases = [Image.open(ASSETS + f).convert("RGBA") for f in FILES]


def rect(name):
    f = frames[name]["frame"]
    return f["x"], f["y"]


def put(atlas, a, x, y):
    """Colle une tuile (TILE x TILE x 4) et ses marges (répétition)."""
    padded = np.pad(np.clip(a, 0, 255).astype(np.uint8), ((MARGIN, MARGIN), (MARGIN, MARGIN), (0, 0)), mode="wrap")
    atlas.paste(Image.fromarray(padded, "RGBA"), (x - MARGIN, y - MARGIN))


def tile(atlas, name):
    x, y = rect(name)
    return np.asarray(atlas.crop((x, y, x + TILE, y + TILE))).astype(np.float64)


# --- 1. Croûte de sel ---

rng = np.random.default_rng(9307)
u = (np.arange(TILE) + 0.5) / TILE
X, Y = np.meshgrid(u, u)


def periodic_noise(cells, seed):
    """Bruit de valeur répétable (0..1), `cells` cellules par côté."""
    g = np.random.default_rng(seed).random((cells, cells))
    fx, fy = X * cells, Y * cells
    ix, iy = np.floor(fx).astype(int), np.floor(fy).astype(int)
    tx, ty = fx - ix, fy - iy
    tx, ty = tx * tx * (3 - 2 * tx), ty * ty * (3 - 2 * ty)
    a, b = g[iy % cells, ix % cells], g[iy % cells, (ix + 1) % cells]
    c, d = g[(iy + 1) % cells, ix % cells], g[(iy + 1) % cells, (ix + 1) % cells]
    return (a * (1 - tx) + b * tx) * (1 - ty) + (c * (1 - tx) + d * tx) * ty


def fbm(octaves, base, seed):
    total, amp, norm = 0.0, 1.0, 0.0
    for o in range(octaves):
        total = total + amp * periodic_noise(base * 2 ** o, seed + o)
        norm += amp
        amp *= 0.5
    return total / norm


def voronoi_edges(n, seed):
    """Distance (en fraction de tuile) au bord de cellule le plus proche
    (F2 - F1 / 2) d'un pavage de Voronoï répétable à `n` germes (réguliers
    et perturbés : polygones de taille voisine, comme les croûtes de sel)."""
    r = np.random.default_rng(seed)
    side = int(round(np.sqrt(n)))
    gx, gy = np.meshgrid((np.arange(side) + 0.5) / side, (np.arange(side) + 0.5) / side)
    pts = np.stack([gx.ravel(), gy.ravel()], 1) + (r.random((side * side, 2)) - 0.5) * 0.75 / side
    pts %= 1.0
    # Bords légèrement sinueux : coordonnées perturbées par un bruit.
    wx = X + 0.012 * (fbm(3, 6, seed + 10) - 0.5)
    wy = Y + 0.012 * (fbm(3, 6, seed + 20) - 0.5)
    f1 = np.full(X.shape, 9.0)
    f2 = np.full(X.shape, 9.0)
    for px, py in pts:
        for ox in (-1, 0, 1):
            for oy in (-1, 0, 1):
                d = np.hypot(wx - (px + ox), wy - (py + oy))
                f2 = np.where(d < f1, f1, np.minimum(f2, d))
                f1 = np.minimum(f1, d)
    return (f2 - f1) / 2


# Une tuile couvre 4 blocs : polygones de ~1 à 1,5 m.
edge = voronoi_edges(12, 9310)
ridge = np.exp(-(edge / 0.02) ** 2)            # arête relevée
crack = np.exp(-(edge / 0.0025) ** 2)           # fente au sommet de l'arête
grain = fbm(4, 64, 9320)                        # cristaux
dome = fbm(3, 4, 9330)                          # plaques un peu bombées
dust = fbm(4, 3, 9340)                          # poussière brune
height = 0.35 * dome + 0.55 * ridge - 0.25 * crack + 0.08 * grain
height = (height - height.min()) / (height.max() - height.min())

white = np.array([238.0, 236.0, 229.0])
color = white * (0.84 + 0.1 * grain[..., None] + 0.12 * ridge[..., None])
tan = np.array([196.0, 184.0, 160.0])
dusty = np.clip((dust - 0.55) * 3.0, 0.0, 1.0) * (1.0 - ridge) * 0.45
color = color * (1 - dusty[..., None]) + tan * dusty[..., None]
color *= (1.0 - 0.12 * crack)[..., None]
salt_color = np.dstack([color, np.full(X.shape, 255.0)])

gy_, gx_ = np.gradient(height * 22.0)
n = np.dstack([-gx_, gy_, np.ones_like(height)])
n /= np.linalg.norm(n, axis=2, keepdims=True)
salt_normal = np.dstack([(n + 1) * 127.5, np.full(X.shape, 255.0)])

rough = 0.72 + 0.12 * ridge + 0.08 * grain
salt_mr = np.dstack([height * 255, rough * 255, np.zeros_like(height), np.full(X.shape, 255.0)])

if "salt.png" not in frames:
    w, h = meta["meta"]["size"]["w"], meta["meta"]["size"]["h"]
    rows = max(f["frame"]["y"] for f in frames.values()) // SLOT + 1
    y0 = rows * SLOT + MARGIN
    nh = -(-(y0 + TILE + MARGIN) // PAD) * PAD
    for k, name in enumerate(FILES):
        fill = (128, 128, 255, 255) if name == "atlas_texture_normal.png" else (0, 0, 0, 0)
        grown = Image.new("RGBA", (w, max(h, nh)), fill)
        grown.paste(atlases[k], (0, 0))
        atlases[k] = grown
    meta["meta"]["size"]["h"] = max(h, nh)
    frames["salt.png"] = {"frame": {"x": MARGIN, "y": y0, "w": TILE, "h": TILE}}
    print(f"rangée ajoutée : salt.png en (32, {y0}), atlas {w}x{max(h, nh)}")
sx, sy = rect("salt.png")
put(atlases[0], salt_color, sx, sy)
put(atlases[1], salt_normal, sx, sy)
put(atlases[2], salt_mr, sx, sy)


# --- 2. Cartes de hauteur ---

k = np.fft.fftfreq(TILE) * 2 * np.pi
KX, KY = np.meshgrid(k, k)
K2 = KX ** 2 + KY ** 2
K2[0, 0] = 1.0


def integrate(gx, gy):
    """Hauteur dont le gradient approche (gx, gy) (tuile répétable)."""
    H = (-1j * KX * np.fft.fft2(gx) - 1j * KY * np.fft.fft2(gy)) / K2
    H[0, 0] = 0.0
    return np.real(np.fft.ifft2(H))


def stretch(a):
    lo, hi = np.percentile(a, 1), np.percentile(a, 99)
    return np.clip((a - lo) / max(hi - lo, 1e-6), 0.0, 1.0)


def highpass(a, cells=8):
    """Retire les variations plus larges que 1/`cells` de tuile (flou FFT)."""
    F = np.fft.fft2(a)
    sigma = 2 * np.pi * cells / TILE
    return np.real(np.fft.ifft2(F * (1 - np.exp(-K2 / (2 * sigma ** 2)))))


for name in HEIGHT_TILES:
    if name == "salt.png":
        continue
    x, y = rect(name)
    col = tile(atlases[0], name)
    nor = tile(atlases[1], name) / 127.5 - 1.0
    nz = np.maximum(nor[..., 2], 0.2)
    gx, gy = -nor[..., 0] / nz, -nor[..., 1] / nz
    luma = highpass(col[..., :3] @ np.array([0.3, 0.59, 0.11]))
    # Convention de la carte de normales (Y vers le haut ou le bas) : celle
    # dont la hauteur ressemble le plus à la luminance.
    a, b = highpass(integrate(gx, gy)), highpass(integrate(gx, -gy))
    corr = lambda h: np.corrcoef(h.ravel(), luma.ravel())[0, 1]
    from_normals, c = (a, corr(a)) if corr(a) >= corr(b) else (b, corr(b))
    # Normales sans rapport avec l'image (gravier) : luminance seule.
    k = 0.6 if c > 0.1 else 0.0
    h = stretch(k * stretch(from_normals) + (1 - k) * stretch(luma))
    mr = tile(atlases[2], name)
    mr[..., 0] = h * 255
    put(atlases[2], mr, x, y)
    print(f"{name:14s} corrélation normales/luminance DX {corr(a):+.2f} GL {corr(b):+.2f}")

for k, name in enumerate(FILES):
    atlases[k].save(ASSETS + name)
json.dump(meta, open(JSON, "w"), indent=2)
print("atlas enregistrés")
