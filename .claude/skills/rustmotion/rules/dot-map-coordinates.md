# Dot Map Coordinates

## Use real lat/lng for data points

The `dot_map` component uses geographic coordinates, not normalized 0-1 values.

### Common city coordinates
| City | lat | lng |
|------|-----|-----|
| New York | 40.71 | -74.01 |
| San Francisco | 37.77 | -122.42 |
| London | 51.51 | -0.13 |
| Paris | 48.86 | 2.35 |
| Berlin | 52.52 | 13.40 |
| Moscow | 55.75 | 37.62 |
| Dubai | 25.20 | 55.27 |
| Mumbai | 19.08 | 72.88 |
| Singapore | 1.35 | 103.82 |
| Tokyo | 35.68 | 139.69 |
| Seoul | 37.57 | 126.98 |
| Beijing | 39.90 | 116.40 |
| Sydney | -33.87 | 151.21 |
| São Paulo | -23.55 | -46.63 |

### Sizing recommendations
- Full-width map: `"size": { "width": 1760, "height": 880 }` with `dot_spacing: 10`, `dot_radius: 2`
- Medium map: `"size": { "width": 800, "height": 400 }` with `dot_spacing: 8`, `dot_radius: 1.5`
- Use `pulse: true` on 3-5 key points maximum to avoid visual clutter

### BAD: using 0-1 coordinates
```json
{ "x": 0.25, "y": 0.35 }
```

### GOOD: using real lat/lng
```json
{ "lat": 40.71, "lng": -74.01, "label": "NYC" }
```
