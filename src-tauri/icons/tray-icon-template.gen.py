"""Generate the monochrome macOS menu-bar icon from the Dextra app icon.

Run after editing icon.svg:

    python3 src-tauri/icons/tray-icon-template.gen.py

Requires Pillow and the project's Tauri CLI.

AppKit uses only the PNG alpha channel for a template image. The white app
card and its shadow must stay transparent in the menu bar, while the D and
code mark form the tinted silhouette.
"""

from pathlib import Path
import subprocess
import tempfile
import xml.etree.ElementTree as ET

from PIL import Image


ICON_DIR = Path(__file__).parent
CANVAS_SIZE = (52, 44)
GLYPH_SIZE = (38, 38)


def main() -> None:
    # Render only the vector mark, excluding the card and both shadows.
    source = ET.parse(ICON_DIR / "icon.svg").getroot()
    namespace = "{http://www.w3.org/2000/svg}"
    mark = ET.Element("svg", {
        "xmlns": "http://www.w3.org/2000/svg",
        "viewBox": source.attrib["viewBox"],
        "width": source.attrib["width"],
        "height": source.attrib["height"],
    })
    for path in source.iter(f"{namespace}path"):
        attrs = dict(path.attrib)
        attrs["fill"] = "#000000"
        ET.SubElement(mark, "path", attrs)

    with tempfile.TemporaryDirectory() as tmp:
        tmp_dir = Path(tmp)
        svg_path = tmp_dir / "tray-mark.svg"
        ET.ElementTree(mark).write(svg_path, encoding="unicode")
        subprocess.run(
            ["pnpm", "tauri", "icon", str(svg_path), "-o", str(tmp_dir), "--png", "512"],
            cwd=ICON_DIR.resolve().parents[1],
            check=True,
            capture_output=True,
        )
        with Image.open(tmp_dir / "512x512.png") as rendered:
            mark_alpha = rendered.convert("RGBA").getchannel("A")
    bounds = mark_alpha.getbbox()
    if bounds is None:
        raise ValueError("Dextra mark is missing from icon.svg")

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
