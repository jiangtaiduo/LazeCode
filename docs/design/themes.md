# Theme skins

LazeCode ships 20 designer skins plus the original **Studio** palette. A skin
has a dark and a light variant; `Settings::dark_skin` and
`Settings::light_skin` remember the choice per mode (only the active mode is
shown in Settings → Appearance).

## Skins

| id | Name | Source palette |
|---|---|---|
| `studio` | 石墨 Studio | original LazeCode colours (default) |
| `hydrangea` | 绣球蓝 Hydrangea Blue | hydrangea petals: powder blue, mocha, cream |
| `garden` | 庭园 Garden | garden greens: sage, olive, moss, cream |
| `royal` | 皇家紫 Royal Violet | lilac frost through midnight tyrian |
| `nordic` | 北境 Nordic | blue-grey architecture with oxide red |
| `lantern` | 灯花 Lantern | lantern blossom: ember, apricot, buttermilk |
| `sand-sage` | 沙丘鼠尾草 Sand & Sage | sand, sage and navy |
| `plum-ridge` | 梅岭 Plum Ridge | misty plum ridge at dusk |
| `maple` | 枫桥 Maple Bridge | river ink, maple rust, autumn paper |
| `reeds` | 金苇 Golden Reeds | golden reeds by the river |
| `violet-dusk` | 紫暮山 Violet Dusk | violet mountains at dusk |
| `lotus` | 荷风 Lotus | lotus pond: ink green, blush, ivory |
| `jade` | 叠翠 Jade Peaks | layered jade peaks and soft gold |
| `riviera` | 里维埃拉 Riviera | riviera blue, terracotta, ivory |
| `iris` | 鸢尾田 Iris Field | iris purple, crocus petal, olive |
| `cacao` | 可可 Cacao | cacao, roasted chestnut, oat milk |
| `moss-gold` | 苔金 Moss & Gold | moss, linden, dry gold |
| `coral` | 珊瑚海岸 Coral Coast | seashell blush, coral, lavender tide |
| `teal-lemon` | 青柠海湾 Teal & Lemon | petrol, teal, lemon, cream |
| `hibiscus` | 木槿 Hibiscus | hibiscus red, leaf green, sand |
| `wild-olive` | 野橄榄 Wild Olive | ivory, saffron, petrol, olive |

`ThemeSkin` lives in `lazecode-core::settings`; each id is the serde name and
must never change once shipped.

## How a palette is built

`crates/lazecode-ui/src/skins.rs` declares ten seed colours per mode:

```
background, editor, accent,
keyword, type, string, number,
green, amber, rose
```

Everything else is derived in OKLCH:

- **Chrome** (rail, titlebar, sidebar, panel, status bar, dialog surfaces,
  gradient) keeps the background's hue and a capped chroma, with fixed
  lightness offsets so every skin has the same depth hierarchy.
- **Text** tiers (body/secondary/muted) and **syntax tokens** are lightness-
  fitted until they clear a contrast target on the editor or the composited
  glass panel. Chroma is capped for body text and gamut-mapped for saturated
  tokens.
- **Semantics** (green/amber/rose/blue/violet) come from the same seeds and
  feed diff rows, selection, conflict markers and ANSI-16.
- **Terminal** keeps a dark canvas in both modes (PTY semantics); ANSI colours
  are derived from the skin's hues and fitted to the terminal background.
- **Diff rows** in light mode are mixed from the light editor and the skin's
  add/delete colours instead of reusing the old dark Monaco literals.

The unit tests in `skins.rs` build all 42 variants and assert body/secondary/
token/ANSI/button/diff contrast ratios, so a new seed that cannot be made
legible fails CI rather than shipping.

## Adding a skin

1. Add a variant to `ThemeSkin` (core), its `ALL` entry and `id()`.
2. Add a `SkinSpec` arm in `skins.rs` with dark and light seeds.
3. Add the display names in `skins::display_name`.
4. Run `cargo test -p lazecode-ui --lib skins`; then check the palette in
   Settings → Appearance.
