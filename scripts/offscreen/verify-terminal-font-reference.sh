#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT_DIR"

REFERENCE_PATH=${1:-/tmp/zeus-clipboard/paste-20260326-170911-961.png}
OUTPUT_PPM=/tmp/neozeus-terminal-font-reference.ppm

if [[ ! -f "$REFERENCE_PATH" ]]; then
    echo "missing terminal font reference image: $REFERENCE_PATH" >&2
    exit 1
fi

cargo test dump_terminal_font_reference_sample -- --ignored

python - "$REFERENCE_PATH" "$OUTPUT_PPM" <<'PY'
from PIL import Image
import numpy as np
import sys

ref = Image.open(sys.argv[1]).convert('RGB')
cur = Image.open(sys.argv[2]).convert('RGB')

def build_mask(img, box, kind):
    crop = img.crop(box)
    mask = np.zeros((crop.height, crop.width), dtype=np.uint8)
    for y in range(crop.height):
        for x in range(crop.width):
            r, g, b = crop.getpixel((x, y))
            if kind == 'green':
                on = g > r + 60 and g > b + 40 and g > 100
            elif kind == 'gray':
                on = (r + g + b) / 3 > 70 and abs(r - g) < 30 and abs(g - b) < 30 and not (g > r + 60 and g > b + 40 and g > 100)
            elif kind == 'yellow':
                on = r > 130 and g > 140 and g > b + 20
            else:
                raise ValueError(kind)
            if on:
                mask[y, x] = 1
    return mask

def bbox(mask):
    ys, xs = np.nonzero(mask)
    if len(xs) == 0:
        raise SystemExit(1)
    return (int(xs.min()), int(ys.min()), int(xs.max()), int(ys.max()))

def aligned_iou(ref_mask, cur_mask):
    rb = bbox(ref_mask)
    cb = bbox(cur_mask)
    rw, rh = rb[2] - rb[0] + 1, rb[3] - rb[1] + 1
    cw, ch = cb[2] - cb[0] + 1, cb[3] - cb[1] + 1
    w, h = max(rw, cw), max(rh, ch)
    ra = np.zeros((h, w), dtype=np.uint8)
    ca = np.zeros((h, w), dtype=np.uint8)
    rx, ry = (w - rw) // 2, (h - rh) // 2
    cx, cy = (w - cw) // 2, (h - ch) // 2
    ra[ry:ry + rh, rx:rx + rw] = ref_mask[rb[1]:rb[3] + 1, rb[0]:rb[2] + 1]
    ca[cy:cy + ch, cx:cx + cw] = cur_mask[cb[1]:cb[3] + 1, cb[0]:cb[2] + 1]
    inter = np.logical_and(ra == 1, ca == 1).sum()
    union = np.logical_or(ra == 1, ca == 1).sum()
    diff = np.logical_xor(ra == 1, ca == 1).sum()
    return rb, cb, 0.0 if union == 0 else inter / union, int(diff), int(union)

checks = [
    ('INFO', 'green', (0, 0, 600, 60), (0, 0, 600, 60), 0.60),
    ('GRAY', 'gray', (440, 0, 1250, 40), (440, 0, 1250, 40), 0.34),
    ('WARN', 'yellow', (0, 150, 700, 243), (0, 90, 700, 184), 0.50),
]

for label, kind, ref_box, cur_box, threshold in checks:
    ref_mask = build_mask(ref, ref_box, kind)
    cur_mask = build_mask(cur, cur_box, kind)
    rb, cb, iou, diff, union = aligned_iou(ref_mask, cur_mask)
    print(f"{label}_ref_bbox={rb}")
    print(f"{label}_cur_bbox={cb}")
    print(f"{label}_iou={iou:.4f}")
    print(f"{label}_diff_pixels={diff}")
    print(f"{label}_union_pixels={union}")
    print(f"{label}_threshold={threshold:.2f}")
    if iou < threshold:
        raise SystemExit(1)
PY

echo "offscreen terminal font reference verification: PASS"
