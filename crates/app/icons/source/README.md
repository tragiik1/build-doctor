# App icon sources

- `icon.svg`: the icon for 48 px and up.
- `icon-small.svg`: the simplified version for 16–32 px (thicker frames, plain
  lit lens), so the taskbar and title bar stay sharp.

The PNGs and `icon.ico` in `icons/` are rendered from these (Chromium, then
Pillow for the multi-size ICO: 16, 24 and 32 px from the small version; 48,
64, 128 and 256 px from the full one).
