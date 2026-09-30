"""Régénère la carte de feuillage des feuillus (leaf_card.png dans l'atlas) :
rameaux portant de petites feuilles pointues, avec des trous et des teintes
variées, au lieu d'un disque plein de grosses feuilles rondes (aspect
« dessin animé »). Affiche l'octogone englobant la partie opaque, à recopier
dans `LEAF_CARD_OCTAGON` (src/render/tree_mesh.rs).

Usage : python3 tools/gen_leaf_card.py
"""
import json, math, random
import numpy as np
from PIL import Image, ImageDraw, ImageFilter

ATLAS, JSON = "assets/atlas_texture.png", "assets/atlas_texture.json"
TILE, SS = 1024, 4
N = TILE * SS
rng = random.Random(3)

def leaf(d, x, y, angle, length, width, color, light):
    """Feuille lancéolée : deux moitiés (claire / foncée) et une nervure."""
    ca, sa = math.cos(angle), math.sin(angle)
    pts_l, pts_r = [], []
    for i in range(9):
        t = i / 8
        w = width * math.sin(math.pi * t ** 0.8) * 0.5
        px, py = x + ca * length * t, y + sa * length * t
        pts_l.append((px - sa * w, py + ca * w))
        pts_r.append((px + sa * w, py - ca * w))
    tip = (x + ca * length, y + sa * length)
    c1 = tuple(int(255 * min(1, v * light)) for v in color) + (255,)
    c2 = tuple(int(255 * min(1, v * light * 0.8)) for v in color) + (255,)
    d.polygon([(x, y)] + pts_l[1:-1] + [tip], fill=c1)
    d.polygon([(x, y)] + pts_r[1:-1] + [tip], fill=c2)
    d.line([(x, y), tip], fill=tuple(int(255 * v * light * 0.7) for v in color) + (255,), width=SS)

def branch(start, a, length, depth_light, level):
    """Rameau courbe portant des feuilles alternes et, au premier niveau,
    des rameaux secondaires."""
    bend = rng.uniform(-0.6, 0.6)
    pts = []
    for i in range(10):
        t = i / 9
        ang = a + bend * t
        if i == 0:
            pts.append(start)
        else:
            px, py = pts[-1]
            pts.append((px + math.cos(ang) * length / 9, py + math.sin(ang) * length / 9))
    d.line(pts, fill=(66, 50, 32, 255), width=int(SS * (2.5 if level == 0 else 1.5)))
    for i in range(1, 10):
        t = i / 9
        px, py = pts[i]
        ang = a + bend * t
        if level == 0 and i in (3, 5, 7) and rng.random() < 0.8:
            side = rng.choice((-1, 1))
            branch((px, py), ang + side * rng.uniform(0.5, 0.9), length * rng.uniform(0.4, 0.6), depth_light, 1)
        for side in (-1, 1):
            if rng.random() < 0.1:
                continue  # feuille manquante : trous
            la = ang + side * rng.uniform(0.4, 1.1) + rng.uniform(-0.2, 0.2)
            size = N * rng.uniform(0.045, 0.075) * (1.0 - 0.25 * t)
            hue = rng.random()
            color = (0.29 + 0.13 * hue, 0.41 + 0.07 * hue, 0.15 + 0.03 * hue)
            leaf(d, px, py, la, size, size * 0.45, color, depth_light * rng.uniform(0.8, 1.12))
    leaf(d, pts[-1][0], pts[-1][1], a + bend, N * 0.06, N * 0.027, (0.35, 0.46, 0.18), depth_light)

img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
cx, cy = N * 0.5, N * 0.5
# Rameaux, du fond (sombres) vers l'avant (clairs) : profondeur dans la
# touffe. Départs dispersés (pas tous du centre : pas d'étoile).
twigs = 26
for k in range(twigs):
    light = 0.65 + 0.5 * k / (twigs - 1)
    a = rng.uniform(0, math.tau)
    r = N * rng.uniform(0.0, 0.2)
    start = (cx + math.cos(a + 2.5) * r, cy + math.sin(a + 2.5) * r)
    branch(start, a, N * rng.uniform(0.3, 0.4), light, 0)

small = img.resize((TILE, TILE), Image.Resampling.LANCZOS)
a = np.array(small).astype(np.float32) / 255
rgb, alpha = a[..., :3], a[..., 3:]
prem = Image.fromarray((np.concatenate([rgb * alpha, alpha], -1) * 255).astype(np.uint8), "RGBA")
blur = np.array(prem.filter(ImageFilter.GaussianBlur(10))).astype(np.float32) / 255
fill = blur[..., :3] / np.maximum(blur[..., 3:], 1e-4)
fill = np.where(blur[..., 3:] > 1e-3, fill, np.array([0.3, 0.42, 0.16]))
out = np.concatenate([np.where(alpha > 0.02, rgb, fill), alpha], -1)
tile = Image.fromarray((np.clip(out, 0, 1) * 255).astype(np.uint8), "RGBA")

atlas = Image.open(ATLAS).convert("RGBA")
f = json.load(open(JSON))["frames"]["leaf_card.png"]["frame"]
padded = np.pad(np.array(tile), ((16, 16), (16, 16), (0, 0)), mode="edge")
atlas.paste(Image.fromarray(padded, "RGBA"), (int(f["x"]) - 16, int(f["y"]) - 16))
atlas.save(ATLAS)

# Octogone englobant (alpha > 40/255), en coordonnées de tuile.
ys, xs = np.nonzero(np.array(tile)[..., 3] > 40)
u, v = (xs + 0.5) / TILE, (ys + 0.5) / TILE
umin, umax, vmin, vmax = u.min(), u.max(), v.min(), v.max()
smin, smax = (u + v).min(), (u + v).max()
dmin, dmax = (u - v).min(), (u - v).max()
pts = [
    (smin - vmin, vmin), (dmax + vmin, vmin), (umax, umax - dmax), (umax, smax - umax),
    (smax - vmax, vmax), (dmin + vmax, vmax), (umin, umin - dmin), (umin, smin - umin),
]
print("coverage", round(float((np.array(tile)[..., 3] > 128).mean()), 3))
print("const LEAF_CARD_OCTAGON: [[f32; 2]; 8] = [" + ", ".join(f"[{p[0]:.3f}, {p[1]:.3f}]" for p in pts) + "];")
