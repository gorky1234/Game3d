"""Agrandit les trois atlas (couleur, normales, rugosité) jusqu'aux multiples
de PAD px suivants, en ajoutant une bande vide à droite et en bas (aucune
tuile ne bouge ; les UV sont recalculées depuis `meta.size` du JSON).

Pourquoi : la compression BC7 (voir src/texture.rs) travaille par blocs de
4x4 px, pour chaque niveau de mipmap. Avec des côtés multiples de 512, les
niveaux jusqu'au 7e (1/128) restent multiples de 4 ; au-delà de la taille
réelle d'un niveau non multiple de 4, wgpu attendait plus de données que la
bibliothèque de mipmaps n'en produit (plantage au chargement).
`MipmapGeneratorSettings::minimum_mip_resolution` arrête la chaîne avant.

Usage : python3 tools/pad_atlas.py (relançable : sans effet si déjà fait ;
à relancer après tout script qui agrandit l'atlas).
"""
import json
from PIL import Image

Image.MAX_IMAGE_PIXELS = None
PAD = 512
ASSETS = "assets/"
JSON = ASSETS + "atlas_texture.json"

meta = json.load(open(JSON))
w, h = meta["meta"]["size"]["w"], meta["meta"]["size"]["h"]
nw, nh = -(-w // PAD) * PAD, -(-h // PAD) * PAD
if (nw, nh) == (w, h):
    print("déjà aux bonnes dimensions", w, h)
else:
    for name in ("atlas_texture.png", "atlas_texture_normal.png", "atlas_texture_metallic_roughness.png"):
        im = Image.open(ASSETS + name).convert("RGBA")
        # Fond : normale plate pour la carte de normales, transparent sinon.
        fill = (128, 128, 255, 255) if name == "atlas_texture_normal.png" else (0, 0, 0, 0)
        out = Image.new("RGBA", (nw, nh), fill)
        out.paste(im, (0, 0))
        out.save(ASSETS + name)
    meta["meta"]["size"] = {"w": nw, "h": nh}
    json.dump(meta, open(JSON, "w"), indent=2)
    print(f"{w}x{h} -> {nw}x{nh}")
