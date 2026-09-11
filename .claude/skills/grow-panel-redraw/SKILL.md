---
name: grow-panel-redraw
description: In the grow repo a panel section is built once and only its Panel::redraw parts refresh afterwards, so a control whose PRESENCE depends on state a stroke or a tick changes silently never appears - no error, no console message, and the stat beside it updates correctly, which makes the panel look fine. Use when adding a conditional button, field or section to any panel under rust/src/ui/, when a control that should have shown up is missing, and before choosing between redraw_panel and rebuild_panel.
---

# A panel section is built once

`ui/*_panel.rs` has two jobs that look alike and are not:

- **`build(root, app, h)`** runs once per panel change and creates every
  section, note, field and button. Anything wrapped in an `if` is decided
  **here, once**.
- **`Panel::redraw(app)`** runs often and touches only the elements the panel
  kept a handle to - typically one `.stat-grid` it clears and refills.

So a section that reads

```rust
if something_a_stroke_changes(app) {
    rows.push(button("...", ...));
}
```

is deciding on the state at build time and keeping that answer until something
sets `rebuild_panel`.

## Which flag does what

| Flag | What happens | When it is set |
| --- | --- | --- |
| `app.redraw_panel` | `Panel::redraw` only | end of a stroke (`settle`), a tick |
| `app.rebuild_panel` | `build` again, whole panel | a press that changes what the panel offers |

`map_panel::settle` sets `redraw_panel`, not `rebuild_panel`, deliberately:
rebuilding the whole panel on every cell of a drag would tear down the
listeners the drag is running through.

## The failure

**Put the picture back everywhere** on the map page was written as "show it
once a cell has been painted over". It never appeared. The tally right above it
counted the painted cells correctly - that is in `redraw` - so the panel looked
like it was working, and nothing was logged anywhere.

## What to do instead

- Put the control there **all the time** and let a stat say how much it would
  act on. A button that does nothing on an empty set is better than one that is
  missing when it is wanted.
- Or, if it really must come and go, set `rebuild_panel` from whatever changes
  the condition - and check it is not something that happens per pointer move.

Conditions that are safe to decide at build time are the ones only a press can
change: whether a settlement exists, whether a picture has been dropped,
whether a switch is on - each of those already sets `rebuild_panel`.

## Catching it

A browser check has to assert on the **control**, not on the number beside it:

```js
if ((await page.locator('#panel-body .btn:text-is("Put the picture back everywhere")').count()) !== 1) {
  problems.push('nothing on the map page puts the picture back');
}
```

Asserting only on the tally passes while the button is missing. `bun run
check:ui` is what found this; see the `grow-checks` skill for running it.
