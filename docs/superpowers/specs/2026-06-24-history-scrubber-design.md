# History Scrubber Design

## Goal

Make the history controls visibly connected to the first-screen gauges. Live History must sit directly below the CPU, RAM, and swap gauges, and the user must be able to inspect previous local samples without confusing that action with changing the gauge style.

## Design

The dashboard keeps the radial CPU, RAM, and swap gauges as the current-state summary. The Live History panel moves immediately under those gauges. The history view selector controls only the Live History canvas and offers line, area, and heatmap modes. The duplicate bar mode is removed because filesystem and pressure sections already use bars, and another bar chart did not add a distinct workflow.

The Live History panel adds a native range input as the timeline scrubber. While live, the scrubber follows the newest sample. When the user moves the scrubber to an older sample, the dashboard renders that captured snapshot into the main gauges and supporting metric panels. The Live button clears the historical selection and returns the gauges to the newest sample.

## Data Flow

Each `/api/snapshot` response is appended to a browser-local rolling buffer capped at the same length as the chart history. No historical data is written to disk or requested from the backend. If the buffer trims old samples while the user is inspecting history, the selected index is clamped to the remaining buffer.

## Accessibility

The scrubber is a native range input with a label and `aria-valuetext`. The Live button is disabled while already live. The selected historical sample is also marked on the history canvas with a vertical guide so the visual state and gauge values stay connected.
