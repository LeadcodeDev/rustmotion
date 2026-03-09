# Rule: Icon Format Must Be prefix:name (Iconify)

The `icon` component renders icons from the **Iconify** open-source icon framework — over 200,000 icons from 150+ icon sets. Icons are fetched from the Iconify API at render time (requires internet access).

The `icon` field must use the `"prefix:name"` format.

**Browse all available icons:** https://icon-sets.iconify.design/

**GOOD:**
```json
{ "type": "icon", "icon": "lucide:home" }
{ "type": "icon", "icon": "mdi:account-circle" }
{ "type": "icon", "icon": "simple-icons:github" }
```

**BAD:**
```json
{ "type": "icon", "icon": "home" }
{ "type": "icon", "icon": "fa-home" }
```

## Common Icon Sets

| Prefix | Name | Style | Count | Best for |
| --- | --- | --- | --- | --- |
| `lucide` | Lucide | Clean line icons | 1500+ | UI, general purpose |
| `mdi` | Material Design Icons | Filled & outlined | 7000+ | Material UI, Android |
| `heroicons` | Heroicons | Tailwind-style | 300+ | Tailwind projects |
| `ph` | Phosphor | Flexible weights | 7000+ | Modern UI |
| `tabler` | Tabler Icons | Stroke-based | 5000+ | Dashboards |
| `ri` | Remix Icon | Clean dual-tone | 2800+ | Web apps |
| `devicon` | Devicon | Dev tool logos | 800+ | Tech stack |
| `simple-icons` | Simple Icons | Brand logos | 3000+ | Company logos |

## Tips

- Default to `lucide` for general UI icons (clean, consistent style)
- Use `simple-icons` for brand/company logos (e.g. `simple-icons:github`, `simple-icons:docker`)
- Use `devicon` for programming language logos (e.g. `devicon:rust`, `devicon:python`)
- All icons are monochrome and colored via `style.color`
