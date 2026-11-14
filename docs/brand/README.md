# sewa brand

The emblem is a white cat asleep on a server box. The box has a slot on the
left and three dots on the right. A thick ink outline draws both. The wordmark
is "sewa" in Nunito Black, converted to paths.

## Colors

| Part | Color |
| --- | --- |
| Ink: outline, eyes, nose, slot, feet, wordmark on light | `#0B3D3B` deep green |
| Device: the server box and the inner ears | `#A7C97A` sage |
| Cat, and the dots on the box | `#FFFFFF` white |
| Cat shade, at the bottom of the body | `#EDE6D6` cream |
| Halo around the emblem on dark, and the wordmark on dark | `#FAF8F2` ivory |
| Light background | `#FAF8F2` ivory |
| Dark background | `#062826` dark green |

Supporting colors, not used in the emblem:

| Name | On light | On dark |
| --- | --- | --- |
| Teal | `#2F8F7E` | `#66B9A8` |
| Mist | `#E7F3E3` | `#E7F3E3` |

The emblem keeps the same colors on dark. The `-dark` files wrap it in an
ivory halo, like a sticker, so the deep green outline stays visible on the dark
background.

## Files

| File | Use |
| --- | --- |
| `mark.svg`, `mark-dark.svg` | Emblem in color, on a light or a dark background. |
| `mark-mono.svg` | Emblem in `currentColor`: one silhouette with the inner lines, the slot, and the dots cut out. |
| `lockup.svg`, `lockup-dark.svg`, `lockup-mono.svg` | Emblem and wordmark side by side. |
| `lockup-stacked.svg`, `lockup-stacked-dark.svg`, `lockup-stacked-mono.svg` | Emblem above the wordmark. |
| `wordmark.svg` | The wordmark alone, in `currentColor`. |
| `cat-mono.svg` | The cat alone, in `currentColor`. |
| `favicon.svg` | The favicon, a copy of `web/public/favicon.svg`. Keep the two the same. |
| `usage.html` | A page that shows each file on a light and a dark background. Open it in a browser. |

## In the web app

- The shell's brand draws the emblem with `SewaMark`, 32 px high, beside the
  name. `web/src/components/SewaMark.css` gives it the colors above for the
  app's light and dark themes. `docs/kiso.md` records why it replaces Kiso's
  BrandMark.
- `SewaMark` holds the paths of `mark.svg`. Keep the two the same.
- `web/public/favicon.svg` is the same emblem in a square viewBox. The
  ivory halo appears when the system uses a dark theme.

## Use

- Every file is SVG with a viewBox, a transparent background, and no images,
  scripts, IDs, or fonts.
- `currentColor` files take the CSS `color` when inlined. Through `<img>`, they
  are black.
- The emblem is about 8:7. It reads from 32 px high. At 16 px only its shape
  reads, so use it that small only as the favicon.
- The emblem is traced with potrace from the brand reference image, the large
  symbol at the top left of the sewa brand sheet. The crop was upscaled 4x
  (Lanczos), snapped to the four emblem colors, and traced one color at a
  time with the `vector-art` skill's `vectorize.py`. The halo and the mono
  silhouettes are traced from masks made from the same snapped image. The
  traced paths are not edited by hand. To change the emblem, change the
  source image and trace it again.
- Nunito is by the Nunito Project Authors and is under the SIL Open Font
  License 1.1. The files hold the outlines of four letters, not the font.
