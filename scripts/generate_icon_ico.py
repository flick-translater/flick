#!/usr/bin/env python3
from __future__ import annotations

import argparse
import sys
from pathlib import Path


DEFAULT_SIZES = (256, 128, 64, 48, 32, 24, 16)


def parse_args() -> argparse.Namespace:
    repo_root = Path(__file__).resolve().parents[1]
    default_input = repo_root / "src-tauri" / "icons" / "icon.png"
    default_output = repo_root / "src-tauri" / "icons" / "icon.ico"

    parser = argparse.ArgumentParser(
        description="Generate a Windows .ico file from a PNG source image."
    )
    parser.add_argument(
        "--input",
        type=Path,
        default=default_input,
        help=f"Source PNG path. Default: {default_input}",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=default_output,
        help=f"Output ICO path. Default: {default_output}",
    )
    parser.add_argument(
        "--sizes",
        type=int,
        nargs="+",
        default=list(DEFAULT_SIZES),
        help="Icon sizes to embed in the .ico file. Default: 256 128 64 48 32 24 16",
    )
    return parser.parse_args()


def ensure_pillow() -> None:
    try:
        import PIL  # noqa: F401
    except ImportError as exc:
        raise SystemExit(
            "Pillow is required. Install it with: python -m pip install Pillow"
        ) from exc


def normalize_sizes(sizes: list[int]) -> list[tuple[int, int]]:
    normalized = sorted({size for size in sizes if size > 0}, reverse=True)
    if not normalized:
        raise SystemExit("At least one positive icon size is required.")
    return [(size, size) for size in normalized]


def generate_icon(input_path: Path, output_path: Path, sizes: list[tuple[int, int]]) -> None:
    from PIL import Image

    if not input_path.is_file():
        raise SystemExit(f"Input PNG not found: {input_path}")

    output_path.parent.mkdir(parents=True, exist_ok=True)

    with Image.open(input_path) as image:
        image = image.convert("RGBA")
        image.save(output_path, format="ICO", sizes=sizes)


def main() -> int:
    args = parse_args()
    ensure_pillow()

    input_path = args.input.resolve()
    output_path = args.output.resolve()
    sizes = normalize_sizes(args.sizes)

    generate_icon(input_path, output_path, sizes)
    print(f"Generated {output_path} from {input_path}")
    print("Embedded sizes:", ", ".join(f"{width}x{height}" for width, height in sizes))
    return 0


if __name__ == "__main__":
    sys.exit(main())
