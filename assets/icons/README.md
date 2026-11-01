# macOS icon alternatives

- `split-pick.icns`: original blue/red split-pick concept (number 2 from the
  first coloured concept board).
- `split-pick-enamel.icns`: photorealistic clear-enamel treatment.
- Matching `.png` files are 1024×1024 RGBA masters, with transparent space
  outside the rounded ivory tile. Presentation backgrounds and labels are removed.

Both icons include 16, 32, 64, 128, 256, 512 and 1024 pixel representations.
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
