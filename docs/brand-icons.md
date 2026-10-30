# Voice Input icons

The original artwork is `public/voice-input.svg`: a symmetric five-bar voice waveform on a forest-green rounded tile. No font glyphs or external icon services are required. The same source supplies the header, favicon and macOS Dock/Finder icon. The hero waveform uses the same proportions; the settings button is a centered, symmetric SVG gear.

Regenerate the committed native assets using the project's pinned Tauri CLI (after installing frontend dependencies):

```sh
node node_modules/@tauri-apps/cli/tauri.js icon public/voice-input.svg --output /tmp/voice-brand-icons
cp /tmp/voice-brand-icons/32x32.png /tmp/voice-brand-icons/128x128.png /tmp/voice-brand-icons/128x128@2x.png /tmp/voice-brand-icons/icon.icns src-tauri/icons/
cp /tmp/voice-brand-icons/32x32.png public/favicon-32.png
```

Only the platforms configured by this app are committed. The SVG remains editable. UI screenshots in `docs/images/home-*.png` are browser renders of the real frontend with synthetic native state, not macOS speech measurements.
