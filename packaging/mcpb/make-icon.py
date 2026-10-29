"""Génère `icon.png`, l'icône du bundle MCPB (08 §4.2).

Dessin géométrique propre au projet : un fronton à colonnes, ivoire sur bleu nuit. Ni le logo de
Légifrance, ni la Marianne, ni le bleu-blanc-rouge de l'État (08 §9.6) : l'icône ne doit rien
laisser croire à une origine officielle.

Usage (Pillow requis, hors dépendances du dépôt) :
    python3 -I packaging/mcpb/make-icon.py packaging/mcpb/icon.png
"""

import sys

from PIL import Image, ImageDraw

SIZE = 512  # taille finale : 512 x 512, comme le recommande MCPB
SCALE = 4  # sur-échantillonnage pour lisser les bords
NIGHT = (22, 32, 58, 255)
IVORY = (244, 238, 222, 255)
GOLD = (201, 162, 77, 255)


def draw(size: int) -> Image.Image:
    s = size * SCALE
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    def u(v: float) -> int:  # unités sur 512 -> pixels sur-échantillonnés
        return round(v * s / 512)

    # Fond : carré arrondi.
    d.rounded_rectangle([0, 0, s - 1, s - 1], radius=u(96), fill=NIGHT)

    # Fronton (triangle), entablement, quatre colonnes, deux marches.
    d.polygon([(u(256), u(104)), (u(88), u(200)), (u(424), u(200))], fill=IVORY)
    d.polygon([(u(256), u(136)), (u(150), u(192)), (u(362), u(192))], fill=NIGHT)
    d.rectangle([u(96), u(208), u(416), u(232)], fill=IVORY)
    for x in (112, 192, 288, 368):
        d.rectangle([u(x), u(244), u(x + 32), u(380)], fill=IVORY)  # fût
        d.rectangle([u(x - 6), u(238), u(x + 38), u(250)], fill=IVORY)  # chapiteau
        d.rectangle([u(x - 6), u(374), u(x + 38), u(386)], fill=IVORY)  # base
    d.rectangle([u(88), u(398), u(424), u(418)], fill=IVORY)
    d.rectangle([u(72), u(426), u(440), u(448)], fill=IVORY)

    # Filet doré sous le fronton : seul accent de couleur.
    d.rectangle([u(96), u(234), u(416), u(240)], fill=GOLD)

    return img.resize((size, size), Image.LANCZOS)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    draw(SIZE).save(sys.argv[1], format="PNG", optimize=True)
