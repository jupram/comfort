# Comfort Gesture Control

A Chrome/Edge Manifest V3 extension that lets you scroll, click, and navigate the current tab with a small set of camera-detected hand gestures. Processing stays in the extension: the MediaPipe runtime and hand model are packaged locally.

## Gestures

| Gesture | Result |
| --- | --- |
| Hold a thumbs-up | Start control |
| Hold a closed fist | Stop control |
| Extend index + middle fingers and slide down | Scroll down |
| Extend index + middle fingers and slide up | Scroll up |
| Pinch index + middle fingertips once | Click at the center reticle |
| Point the index finger left with the thumb up and other fingers curled | Browser back |

Scrolling, clicking, and browser navigation are ignored until the thumbs-up start gesture has been accepted. A green page-edge halo and an `ON` toolbar badge identify the controlled tab. Changing tabs or closing the side panel stops control automatically.

## Build and load

```powershell
npm.cmd install
npm.cmd run build
```

Then open `chrome://extensions` or `edge://extensions`, enable **Developer mode**, choose **Load unpacked**, and select the generated `dist` directory.

Click the extension toolbar button to open its side panel, enable the camera, and run calibration once. Calibration records your relaxed two-finger spacing and pinch spacing; the scroll-speed slider can be adjusted at any time.

If camera access is blocked, the side panel shows **Open camera permission**. Use it to open the extension's permission page, click **Allow camera**, approve the browser prompt, then return to the side panel and enable the camera again. If Windows blocks camera access for desktop apps, enable it under **Settings > Privacy & security > Camera** first.

## Notes

- Clicks target the fixed reticle in the center of the page; the extension never moves the pointer.
- Browser-internal pages, extension stores, and other protected pages do not permit content scripts and cannot be controlled.
- Synthetic extension clicks work on normal links, buttons, and controls, but sites that explicitly require trusted hardware input may reject them.

## Development

```powershell
npm.cmd run check
```

Source lives in `src/`; extension service-worker and content-script files live in `public/` so Vite copies them without transformation.
