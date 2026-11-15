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
| `error-403.svg`, `error-403-dark.svg` | An angry cat guards the server box: access denied (403). |
| `error-500.svg`, `error-500-dark.svg` | A worried cat with a wrench on a cracked, taped box: the server failed (500, other 5xx, or no answer). |
| `usage.html` | A page that shows each file on a light and a dark background. Open it in a browser. |

## In the web app

- The shell's brand draws the emblem with `SewaMark`, 32 px high, beside the
  name. `web/src/components/SewaMark.css` gives it the colors above for the
  app's light and dark themes. `docs/kiso.md` records why it replaces Kiso's
  BrandMark.
- `SewaMark` holds the paths of `mark.svg`. Keep the two the same.
- `web/public/favicon.svg` is the same emblem in a square viewBox. The
  ivory halo appears when the system uses a dark theme.
- `ErrorPage` shows `error-403` or `error-500` above the status code, a
  title, one line, and one action. The app loads copies from `web/public/`.
  Keep them the same as the files here. Both variants load, and
  `ErrorPage.css` shows the one for the app's theme.

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

## Error illustrations

- Use `error-403` when access is denied, and `error-500` for a 5xx or a
  controller that does not answer. Do not use them for a 404 or an empty list.
- Use one per page, 160 to 240 px high, centered above the copy. Do not use
  them as icons or below 120 px: the small parts stop reading.
- Put the code, title, and text in the page, never in the art. The files have
  no text.
- Pick the file by the background: the light file on light, the `-dark` file
  on dark. Do not recolor them.
- They are traced like the emblem, with the same steps, from the error-page
  reference: two cats over the captions "403" and "500". The captions and a
  mouse cursor in the screenshot were left out: the crop stops above the
  captions, and the cursor was painted over with the box color before the
  trace. Light tones that touch the background, like the ground shadow, were
  dropped. The colors are ink, sage, white, and mist (`#E7F3E3`) for the
  smoke. Mist is kept only in the smoke clouds, which stand apart from the
  cat and the box. Pale antialias pieces inside that shape, like the edges of
  the inner ears, take the cat color. The `-dark` files add the ivory halo.
