# macOS app icon

`icon-01.icns` **is the app icon**: `packaging/build-app.sh` copies it to
`ChordProLive.app/Contents/Resources/AppIcon.icns`. To change the app icon,
point `ICON` in that script at another `.icns` here.

## Alternatives

- `split-pick.icns`: original blue/red split-pick concept (number 2 from the
  first coloured concept board).
- `split-pick-enamel.icns`: photorealistic clear-enamel treatment.
- `split-pick-translucent.icns`: retro marbled blue/red translucent pick
  (number 4 from the retro concept board).
- Matching `.png` files are 1024×1024 RGBA masters, with transparent space
  outside the rounded ivory tile. Presentation backgrounds and labels are removed.

All alternatives include 16, 32, 64, 128, 256, 512 and 1024 pixel representations.
The original concept was extracted from a roughly 380-pixel tile and upscaled;
its 1024-pixel representation does not contain additional source detail.
The enamel source tile was roughly 1016 pixels wide. These exports preserve
raster concept artwork; they are not vector redraws. Neither is installed as
an application default.


Artwork was generated using the built-in image generation tool in the icon
exploration chat. Original concept source: `exec-4fdf053a-2fc1-4c4e-867c-7111233f8275.png`;
enamel source: `exec-2eed244b-735a-4409-bf59-98eaeb1d31db.png`.
Export: crop the selected tile, mask its rounded corners, center it at 824 pixels
wide on a transparent 1024-pixel canvas, and resize with Lanczos filtering.
The ICNS containers store PNG chunks resized with Pillow; all representations decoded
successfully. Apple's `iconutil` rejected the intermediate iconsets on this host.

To regenerate and validate the containers from the masters, run
`python3 assets/icons/build.py` with Pillow installed. The exporter includes
standard and Retina PNG representations in the ICNS container.

The translucent alternative was extracted from the roughly 413-pixel tile in
`exec-048763fb-531e-4ddc-a7aa-2e4cb44a861d.png`, cropped at
(85, 511)–(498, 919), with an 88-pixel rounded-corner mask. It uses the same
824-pixel tile width on a transparent 1024-pixel canvas. Large sizes are
upscaled from the concept board; this export adds no new source detail.

## Electric alternatives

- `split-pick-illuminated-resin.icns`: neon board number 2, lit blue/red resin.
- `split-pick-after-hours.icns`: neon board number 4, glowing CP on black.

Both have matching transparent 1024-pixel PNG masters and standard/Retina
representations, validated by `build.py`. Source board:
`exec-edecc3dd-d1b7-4ea9-80ad-10ec85a61e4c.png`. Tile crops are
(657, 22)–(1189, 533) and (657, 615)–(1189, 1143), respectively, with
112-pixel rounded-corner masks. As with the other board exports, these are
upscaled raster concepts, centered at 824 pixels wide on a 1024-pixel canvas.

## Neon colourways on black

The original marbled resin style with vivid pigments on black tiles:

1. `split-pick-neon-pink-blue.icns`
2. `split-pick-neon-cyan-violet.icns`
3. `split-pick-neon-blue-magenta.icns`
4. `split-pick-neon-lime-pink.icns`

Each has a matching RGBA PNG master and all standard/Retina representations.
These are colour variations, not neon-tube designs. They do not change the
app's default icon. Source board: `exec-1f8c9f28-85e5-45a9-bbda-714af9f56d0c.png`.
Crops (left, top, right, bottom), in order: (93, 37, 598, 534),
(655, 37, 1160, 534), (93, 626, 598, 1126), (655, 626, 1160, 1126).
Each uses a 110-pixel rounded-corner mask, resized with Lanczos filtering to
824 pixels wide and centered on a transparent 1024-pixel canvas. Large sizes
are upscaled from the roughly 505-pixel source tiles.

## Playful material alternatives

1. `split-pick-holographic-backstage.icns`
2. `split-pick-forbidden-gummy.icns`
3. `split-pick-arcade-boss.icns`
4. `split-pick-glitter-bomb.icns`
5. `split-pick-liquid-psychedelia.icns`
6. `split-pick-pocket-universe.icns`

Each has a matching 1024-pixel RGBA PNG master and standard/Retina sizes.
Source board: `exec-312140c5-f35b-4ca1-8db9-60e8e65c90a1.png`.
Tile crops (left, top, right, bottom), in order:
- holographic-backstage: (75, 29, 492, 440)
- forbidden-gummy: (562, 29, 979, 440)
- arcade-boss: (1047, 29, 1464, 440)
- glitter-bomb: (75, 511, 492, 931)
- liquid-psychedelia: (562, 511, 979, 931)
- pocket-universe: (1047, 511, 1479, 931)

Exports use a 90-pixel corner mask, Lanczos resizing to 824 pixels wide, and
transparent 1024-pixel canvases. The cosmic version includes extra space
and a mask extension for its orbital ring. Large sizes are upscaled from
roughly 417-pixel source tiles. These alternatives do not change the app default.
