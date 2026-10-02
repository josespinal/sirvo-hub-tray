# Icon sources

The Recibito from the Sirvo brand guide in Paper ("Sirvo — Guía gráfica"):
- `app.svg`: page "Rebanada 1 — Marca de entrada", board "05 — Ícono, splash y modo oscuro", "Ícono de app · fondo verde".
- `tray-*.svg`: board "06 — Íconos de estado del tray (Sirvo Hub)" on the same page.

The square is 96 px with a 22 px radius, the glyph is 50 px and centred, and only the background changes per state:

| File | State | Background |
|---|---|---|
| `tray-running` | Running | #006B54 (Sirvo green) |
| `tray-stopped` | Stopped | #6B6B6B |
| `tray-error` | Errored, NeedsSetup, ConfigError | #C42B1C (--destructive) |

Regenerate:

```bash
rsvg-convert -w 1024 -h 1024 src-tauri/icons/source/app.svg -o /tmp/app-1024.png
npx tauri icon /tmp/app-1024.png      # icon.png, .ico, .icns, Square*, ios/ (delete android/)
for s in running stopped error; do
  rsvg-convert -w 64 -h 64 src-tauri/icons/source/tray-$s.svg -o src-tauri/icons/tray-$s.png
done
```
