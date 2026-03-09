# Rule: Icon Format Must Be prefix:name

The `icon` field must be an Iconify identifier in `"prefix:name"` format. Requires internet access for fetching from the Iconify API.

Common prefixes: `lucide`, `mdi`, `heroicons`, `ph`, `tabler`, `ri`, `devicon`.

**GOOD:**
```json
{ "type": "icon", "icon": "lucide:home" }
{ "type": "icon", "icon": "mdi:account-circle" }
```

**BAD:**
```json
{ "type": "icon", "icon": "home" }
{ "type": "icon", "icon": "fa-home" }
```
