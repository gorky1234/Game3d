"""Badlands moins « dessin animé » (voir gen_badlands_textures.py, qui a créé
les deux tuiles) :
- red_rock.png (côtés, falaises) : une vraie paroi sédimentaire (Poly Haven
  « cliff_side », CC0 : grès en bancs, fissures, blocs) à la place du sable
  recoloré en bandes peintes, régulières, qui faisaient des gâteaux en
  couches. Normales, rugosité et relief (canal R de la carte « mr ») de la
  même photo. Les grandes strates de couleur viennent du shader
  (terrain.wgsl), en altitude, d'épaisseur irrégulière.
- red_sand.png (dessus) : désaturée et éclaircie (sol poussiéreux, ocre-beige)
  : l'orange vif uniforme était l'autre moitié de l'effet dessin animé.

Usage : python3 tools/gen_badlands_rock.py (relançable : red_sand part
toujours de sa version d'origine, lue dans git, et les photos de
tools/polyhaven/cliff_side_*_2k.png).
"""
import json
import subprocess
import io
import numpy as np
from PIL import Image

Image.MAX_IMAGE_PIXELS = None
ASSETS = "assets/"
FILES = ("atlas_texture.png", "atlas_texture_normal.png", "atlas_texture_metallic_roughness.png")
JSON = ASSETS + "atlas_texture.json"
PH = "tools/polyhaven/cliff_side_{}_2k.png"
TILE, MARGIN = 1024, 32

meta = json.load(open(JSON))
atlases = [Image.open(ASSETS + f).convert("RGBA") for f in FILES]


def frame(name):
    f = meta["frames"][name]["frame"]
    return int(f["x"]), int(f["y"])


def put(atlas, a, x, y):
    """Tuile répétable : marge = contenu enroulé."""
    padded = np.pad(np.clip(a, 0, 255).astype(np.uint8), ((MARGIN, MARGIN), (MARGIN, MARGIN), (0, 0)), mode="wrap")
    atlas.paste(Image.fromarray(padded, "RGBA"), (x - MARGIN, y - MARGIN))


def load(kind):
    return np.array(Image.open(PH.format(kind)).convert("RGB").resize((TILE, TILE), Image.Resampling.LANCZOS)).astype(np.float32)


# --- Paroi : photo, teinte ramenée vers un rouge-ocre de grès ---
diff = load("diff")
luma = diff @ np.array([0.3, 0.59, 0.11])
# Garde la photo (bancs clairs, joints sombres, taches de fer) ; brun-rouge
# de grès, plus sombre et un peu moins saturé que l'original (orange vif à
# côté d'un sol pâle : contraste de dessin animé).
color = diff * np.array([0.92, 0.74, 0.64])
color = luma[..., None] * 0.8 + (color - luma[..., None] * 0.8) * 0.82
rock = np.dstack([color, np.full((TILE, TILE), 255.0)])
x, y = frame("red_rock.png")
put(atlases[0], rock, x, y)
normal = load("nor_gl")
put(atlases[1], np.dstack([normal, np.full((TILE, TILE), 255.0)]), x, y)
rough = load("rough")[..., 0]
disp = load("disp")[..., 0]
# Carte « mr » : R = relief (mélange selon la hauteur), G = rugosité, B = 0.
mr = np.dstack([disp, rough, np.zeros((TILE, TILE)), np.full((TILE, TILE), 255.0)])
put(atlases[2], mr, x, y)

# --- Sol : désaturé, éclairci (poussière) ---
x, y = frame("red_sand.png")
# Toujours depuis la version d'origine (git), pour que le script soit
# relançable sans désaturer deux fois.
original = Image.open(io.BytesIO(subprocess.run(["git", "show", "f6d32cd:" + ASSETS + FILES[0]], capture_output=True, check=True).stdout)).convert("RGBA")
sand = np.array(original.crop((x, y, x + TILE, y + TILE))).astype(np.float32)
rgb = sand[..., :3]
l = rgb @ np.array([0.3, 0.59, 0.11])
rgb = l[..., None] + (rgb - l[..., None]) * 0.7
# Ocre-beige plutôt que rose : un peu de jaune, moins de rouge.
rgb = rgb * np.array([1.06, 1.06, 0.92]) + np.array([4.0, 6.0, 2.0])
put(atlases[0], np.dstack([rgb, sand[..., 3]]), x, y)

for im, f in zip(atlases, FILES):
    im.save(ASSETS + f)
print("red_rock et red_sand mises à jour")
