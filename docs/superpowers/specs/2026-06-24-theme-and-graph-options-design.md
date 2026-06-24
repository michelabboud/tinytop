# Theme and Graph Options Design

## Goal

Add user-selectable visual themes and multiple graph presentations to the standalone TinyTop.

## Design

The dashboard keeps the existing operations cockpit layout and adds a display-controls band below the topbar. The first segmented control selects a theme. The second segmented control selects how the live history canvas renders the same rolling data.

## Themes

- Midnight: original dark operations palette.
- Matrix: high-contrast green terminal palette.
- Aurora: cool teal, blue, and violet palette.
- Solar: light professional dashboard palette.
- Ember: dark warm orange and rose palette.

All themes use CSS custom properties so panels, controls, gauges, bars, and canvas colors stay coherent.

## Graph Modes

- Line: precise time-series view for CPU, RAM, swap, and load.
- Area: streaming area emphasis for overall pressure shape.
- Bars: compact recent-sample comparison.
- Heatmap: intensity grid for scanning spikes across series.

The data source does not change between modes.

## Accessibility

Controls are native buttons grouped in fieldsets with visible legends. Active choices use `aria-pressed`. Graphs keep visible numeric KPIs elsewhere on the page, so color is not the only way to read current values.
