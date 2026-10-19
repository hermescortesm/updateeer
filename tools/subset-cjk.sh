#!/bin/sh
# Rebuilds assets/fonts/NotoSansCJK{jp,sc}-Subset.otf: Noto Sans CJK (SIL OFL) cut down to the
# characters used in src/i18n.rs. Run it whenever Japanese or Chinese texts change; the test
# `embedded_fonts_draw_the_language_names` fails when a glyph is missing.
# Needs: pip install fonttools
set -e
cd "$(dirname "$0")/.."
work=$(mktemp -d)
base=https://github.com/notofonts/noto-cjk/raw/main/Sans/OTF
curl -fL -o "$work/jp.otf" "$base/Japanese/NotoSansCJKjp-Regular.otf"
curl -fL -o "$work/sc.otf" "$base/SimplifiedChinese/NotoSansCJKsc-Regular.otf"
python3 - "$work/chars.txt" <<'P'
import sys
s = open('src/i18n.rs', encoding='utf-8').read()
chars = {c for c in s if ord(c) > 0x2E7F or c in '→—“”«»‹›…·–'}
chars.update('0123456789%()[]{}:;,.!?/-+ ·')
open(sys.argv[1], 'w', encoding='utf-8').write(''.join(sorted(chars)))
P
for f in jp sc; do
  pyftsubset "$work/$f.otf" --text-file="$work/chars.txt" --output-file="assets/fonts/NotoSansCJK$f-Subset.otf" --layout-features='*' --no-hinting --desubroutinize
done
rm -rf "$work"
