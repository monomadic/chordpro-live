"""Rebuild the ICNS containers from their RGBA PNG masters (requires Pillow)."""
from pathlib import Path
import io
import struct
from PIL import Image

ROOT = Path(__file__).resolve().parent
CHUNKS = ((b'icp4', 16), (b'icp5', 32), (b'icp6', 64),
          (b'ic07', 128), (b'ic08', 256), (b'ic09', 512), (b'ic10', 1024),
          (b'ic11', 32), (b'ic12', 64), (b'ic13', 256), (b'ic14', 512))
for name in ('split-pick', 'split-pick-enamel', 'split-pick-translucent',
             'split-pick-illuminated-resin', 'split-pick-after-hours',
             'split-pick-neon-pink-blue', 'split-pick-neon-cyan-violet',
             'split-pick-neon-blue-magenta', 'split-pick-neon-lime-pink',
             'split-pick-holographic-backstage',
             'split-pick-forbidden-gummy',
             'split-pick-arcade-boss',
             'split-pick-glitter-bomb',
             'split-pick-liquid-psychedelia',
             'split-pick-pocket-universe',
             'split-pick-backstage-pass',
             'split-pick-candy-live',
             'split-pick-arcade-live',
             'split-pick-glitter-encore',
             'split-pick-psychedelic-live',
             'split-pick-cosmic-live'):
    master = Image.open(ROOT / f'{name}.png')
    assert master.size == (1024, 1024) and master.mode == 'RGBA'
    chunks = []
    for tag, size in CHUNKS:
        out = io.BytesIO()
        master.resize((size, size), Image.Resampling.LANCZOS).save(out, format='PNG')
        payload = out.getvalue()
        chunks.append(tag + struct.pack('>I', len(payload) + 8) + payload)
    payload = b''.join(chunks)
    destination = ROOT / f'{name}.icns'
    destination.write_bytes(b'icns' + struct.pack('>I', len(payload) + 8) + payload)
    with Image.open(destination) as icon:
        sizes = icon.info['sizes']
        assert {16, 32, 64, 128, 256, 512, 1024} <= {w*s for w, h, s in sizes}
        for size in sizes:
            decoded = icon.icns.getimage(size)
            decoded.load()
            assert decoded.size == (size[0]*size[2], size[1]*size[2])
    print(f'{destination.name}: decoded all {len(sizes)} representations')
