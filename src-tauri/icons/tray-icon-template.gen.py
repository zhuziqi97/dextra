"""Generate the monochrome macOS menu-bar icon from the Dextra app icon.

Run after regenerating icon.png from icon.svg:

    python3 src-tauri/icons/tray-icon-template.gen.py

Requires Pillow.

AppKit uses only the PNG alpha channel for a template image. The white app
card and its pale shadow must stay transparent in the menu bar, while the
blue D and code mark form the tinted silhouette.
"""

from pathlib import Path

from PIL import Image, ImageChops


ICON_DIR = Path(__file__).parent
CANVAS_SIZE = (52, 44)
GLYPH_SIZE = (38, 38)


def main() -> None:
    source = Image.open(ICON_DIR / "icon.png").convert("RGBA")
    red, _, blue, source_alpha = source.split()

    # The supplied icon's cyan/blue mark has substantially more blue than red.
    # The white card and its gray-blue shadow do not cross this threshold.
    blue_delta = ImageChops.subtract(blue, red)
    mark_alpha = blue_delta.point(lambda value: min(255, max(0, (value - 40) * 7)))
    mark_alpha = ImageChops.multiply(mark_alpha, source_alpha)
    bounds = mark_alpha.getbbox()
    if bounds is None:
        raise ValueError("Dextra mark is missing from icon.png")

    mark_alpha = mark_alpha.crop(bounds)
    mark_alpha.thumbnail(GLYPH_SIZE, Image.LANCZOS)
    alpha = Image.new("L", CANVAS_SIZE, 0)
    alpha.paste(
        mark_alpha,
        ((CANVAS_SIZE[0] - mark_alpha.width) // 2, (CANVAS_SIZE[1] - mark_alpha.height) // 2),
    )

    output = Image.new("RGBA", CANVAS_SIZE, (0, 0, 0, 0))
    output.putalpha(alpha)
    path = ICON_DIR / "tray-icon-template.png"
    output.save(path)
    print(f"wrote {path} {output.size}")


if __name__ == "__main__":
    main()
