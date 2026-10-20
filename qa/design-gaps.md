# Fluent design gaps, against the WinUI 3 Gallery

Reference: the WinUI 3 Gallery (Microsoft Store `9P3JFPWWDZRC`, source
github.com/microsoft/WinUI-Gallery; its screenshot is in that repository's
`.github/assets/Screenshot-light.png` and is not copied here). The values
used come from WinUI's own control templates (microsoft-ui-xaml,
`*_themeresources.xaml`). dm as it is: `docs/screenshot.png` and
`docs/screenshot-dark.png`, rendered by the `shots` window test.

## Fixed

| Gap | Was | Now | Commit |
|---|---|---|---|
| Colours | dm's own blue palette | WinUI's light and dark brushes (base, layer, dialog, text, strokes, accent, subtle fills) | c0b450c |
| Dialogs | egui's grey centred title bar, buttons at the left | ContentDialog: title at the top left in 20 px semibold, 24 px padding, buttons at the right on a footer band, smoke behind | c0b450c, 576ae61, c84913d |
| Controls | egui's check boxes, fields, drop-downs, tabs | WinUI's: 20 px boxes, a field's bottom line, chevrons, the selector bar, radio buttons | c0b450c, 576ae61 |
| List selection | A solid blue row | A subtle fill and the accent pill; rows that meet at 32 px | c0b450c, c84913d |
| Navigation pane | A tree with filled triangles at the left | Expandable items, chevron at the right, each place its own icon | fbd4342, bfd3b32 |
| Content area | A card with margins | A layer reaching the right edge, its top-left corner rounded | fbd4342 |
| Typeface | Inter | Segoe UI from Windows, Selawik elsewhere | f847c11 |
| Icons | dm's drawn outlines | Fluent UI System Icons | bfd3b32 |

## Open

| Gap | What the Gallery has | Why dm does not yet |
|---|---|---|
| Mica and Acrylic | A see-through backdrop and flyouts | Windows 11 only; dm has never run on Windows |
| Title bar | Its own, holding the back button and search | dm keeps the system's |
| Accent colour | The person's own | dm uses Windows' default blue |
| High contrast | A high-contrast look | None |
| Settings | Grouped in cards, on/off as ToggleSwitches | Options uses headings and check boxes |
| Status and errors | InfoBars | A list of problems and the status line |
| Settings in the pane | At the pane's foot | In the menu and the toolbar |
| Smaller things | | gap 71 in `docs/gaps.md`: number boxes' line, menu item padding, tooltips' radius, tabs overflowing at 150 % |
