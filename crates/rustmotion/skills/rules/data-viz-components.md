# Data Visualization Component Selection

## Quick decision tree

- **Single KPI number** → `stat` (with trend + sparkline)
- **Progress toward goal** → `gauge` (semi-circle) or `progress` (linear/circular)
- **Inline trend indicator** → `sparkline` (inside card or standalone)
- **Full data comparison** → `chart` (12 types available)
- **Loading state** → `skeleton` (rectangle/circle/text variants)
- **Data table** → `table` (with column_widths, column_align)

## Gauge vs Progress (circular)

- **Gauge**: semi-arc (135°–405°), shows a value with label, best for dashboard KPIs like CPU/memory
- **Progress circular**: full 360° ring, shows percentage, best for completion tracking

## Sparkline vs Line chart

- **Sparkline**: no axes, no labels, compact (120x40 default), inline use
- **Line chart**: axes, grid, labels, larger, standalone data viz

## Skeleton loading pattern

Use skeleton variants to match the content they replace:
```json
{ "type": "skeleton", "variant": "circle", "style": { "width": 64, "height": 64 } }
{ "type": "skeleton", "variant": "text", "lines": 3 }
{ "type": "skeleton", "variant": "rectangle", "style": { "width": 400, "height": 200 } }
```

## Combining components for dashboards

A dashboard scene typically uses:
1. `stat` cards in a row (KPIs)
2. `chart` components (area/bar/donut) for detailed data
3. `table` for tabular data
4. `gauge` or `progress` for single metrics
5. `sparkline` inline within cards for trends
