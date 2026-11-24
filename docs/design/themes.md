# Theme pairs

LazeCode ships **seven curated theme pairs**. One selection applies to both
modes; `Settings::theme` chooses the active variant. The old independent
`dark_skin` / `light_skin` keys are read once and migrated, and every legacy
skin id remains a serde alias so existing `settings.toml` files keep loading.

| id | 中文 | English | Character |
|---|---|---|---|
| `graphite` | 石墨 | Graphite | cool neutral chrome, azure accent (default) |
| `rice-paper` | 宣纸 | Rice Paper | warm paper by day, cool ink by night, cinnabar accent |
| `tundra` | 苔原 | Tundra | grey-green surfaces, moss accent |
| `abyss` | 深海 | Abyss | blue-black surfaces, cyan accent |
| `iris` | 鸢尾 | Iris | violet-grey surfaces, iris accent |
| `amber` | 琥珀 | Amber | warm brown surfaces, golden accent |
| `rosewood` | 绯木 | Rosewood | rose-tinted surfaces, crimson accent |

`ThemeSkin` lives in `lazecode-core::settings`; canonical ids are the settings
contract and must never change. Legacy ids map as follows:

| legacy ids | theme |
|---|---|
| `studio` | `graphite` |
| `hydrangea`, `nordic`, `riviera`, `teal-lemon` | `abyss` |
| `garden`, `jade`, `lotus`, `sand-sage` | `tundra` |
| `royal`, `violet-dusk` | `iris` |
| `lantern`, `maple`, `cacao`, `reeds`, `moss-gold`, `wild-olive` | `amber` |
| `coral`, `hibiscus`, `plum-ridge` | `rosewood` |

## How a palette is built

`crates/lazecode-ui/src/skins.rs` declares only a neutral hue/chroma, the exact
accent, and a small syntax temperature shift per mode. Everything else uses a
shared, fixed ladder:

| mode | panel | canvas | editor | raised | border | text / secondary / muted |
|---|---:|---:|---:|---:|---:|---:|
| dark | 0.19 | 0.22 | 0.245 | 0.285 | 0.36 | 0.935 / 0.74 / 0.615 |
| light | 0.925 | 0.952 | 0.988 | 0.995 | 0.855 | 0.235 / 0.445 / 0.55 |

- Surfaces are **opaque**; the window has no full-body gradient. Depth comes
  from the ladder plus 1px borders, not translucent panels.
- Accent is reserved for primary actions, focus, selection and active-tab
  indicators. Menu hover and ordinary rows use neutral overlays.
- Semantic roles are fixed across themes: success green, warning amber,
  danger red, info blue. Theme changes only lightness/chroma.
- Syntax roles are fixed: keyword violet, type/function blue, string green,
  number amber, comment muted. A theme shifts the hue by at most a few degrees.
- Diff rows use the semantic green/red tints, never the theme accent.
- The terminal follows the active mode. Dark mode uses a deep terminal canvas;
  light mode uses a light canvas with a light-mode ANSI-16. The 256-colour cube
  and grayscale ramp are darkened on light terminals when contrast is too low.

The unit tests in `skins.rs` build all 14 variants and assert body/secondary/
token/ANSI/button/diff contrast ratios plus a minimum pairwise distance, so a
near-duplicate or illegible theme fails CI rather than shipping.

## Adding a theme

1. Add a variant to `ThemeSkin` (core), its `ALL` entry and `id()`.
2. Add a `SkinSpec` arm in `skins.rs` with dark and light `ModeSpec`s.
3. Add the display names in `skins::display_name`.
4. Run `cargo test -p lazecode-ui --lib skins`; then render every variant with
   `LAZE_SKIN=<id> LAZE_THEME=dark|light` on the snapshot harness.
