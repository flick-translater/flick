#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path

from PIL import Image


REPO_ROOT = Path(__file__).resolve().parents[1]
SOURCE = REPO_ROOT / "src-tauri" / "icons" / "icon.png"
WINDOWS_PNG = REPO_ROOT / "src-tauri" / "icons" / "icon_windows.png"
WINDOWS_ICO = REPO_ROOT / "src-tauri" / "icons" / "icon.ico"
TARGET_SIZE = 1024
ICO_SIZES = [
    (256, 256),
]
BACKGROUND_THRESHOLD = 36


def is_background(pixel: tuple[int, int, int, int]) -> bool:
    r, g, b, a = pixel
    return a == 0 or (r <= BACKGROUND_THRESHOLD and g <= BACKGROUND_THRESHOLD and b <= BACKGROUND_THRESHOLD)


def main() -> int:
    image = Image.open(SOURCE).convert("RGBA")
    # Remove the dark backdrop so Windows can use the icon silhouette directly.
    cleaned = []
    for (r, g, b, a) in image.getdata():
        if is_background((r, g, b, a)):
            cleaned.append((r, g, b, 0))
        else:
            cleaned.append((r, g, b, a))
    image.putdata(cleaned)

    bbox = image.getbbox()
    if bbox is None:
        raise SystemExit("No visible pixels found after removing the background.")

    cropped = image.crop(bbox)

    # Clear any remaining dark fringe introduced by the original black matte.
    fringe_cleaned = []
    for (r, g, b, a) in cropped.getdata():
        if a != 0 and r <= BACKGROUND_THRESHOLD and g <= BACKGROUND_THRESHOLD and b <= BACKGROUND_THRESHOLD:
            fringe_cleaned.append((r, g, b, 0))
        else:
            fringe_cleaned.append((r, g, b, a))
    cropped.putdata(fringe_cleaned)

    recrop = cropped.getbbox()
    if recrop is None:
        raise SystemExit("Icon became empty after fringe cleanup.")
    cropped = cropped.crop(recrop)

    # Add a small transparent margin so the rounded square is close to the edges
    # without touching them, which reads better in the Windows taskbar.
    margin = int(max(cropped.width, cropped.height) * 0.04)
    square_side = max(cropped.width, cropped.height) + margin * 2
    canvas = Image.new("RGBA", (square_side, square_side), (0, 0, 0, 0))
    x = (square_side - cropped.width) // 2
    y = (square_side - cropped.height) // 2
    canvas.alpha_composite(cropped, (x, y))

    final_png = canvas.resize((TARGET_SIZE, TARGET_SIZE), Image.Resampling.LANCZOS)
    final_png.save(WINDOWS_PNG, format="PNG")
    final_png.save(WINDOWS_ICO, format="ICO", sizes=ICO_SIZES)

    print(f"Generated {WINDOWS_PNG}")
    print(f"Generated {WINDOWS_ICO}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
