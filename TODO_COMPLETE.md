# Done

Newest first. One line each; the reasoning lives in ARCHITECTURE.md and the
README.

- (LLM) Move people: a switch above the map turns a press on a settler into
  picking them up. The pointer carries them, letting go puts them down on the
  nearest ground they can stand in, what they were doing is given up the way any
  change of plan gives it up, and the tick leaves them out while they are held.
- (LLM) Lamp posts, built like anything else, lighting a pool of ground after
  dark by drawing over the night tint rather than cutting holes in it.
- (LLM) The fullscreen escape hatch fades out when nothing has moved for a
  couple of seconds.
- (LLM) Settlers with no bed walk back to town before lying down, and turning in
  is its own motion beside sleeping.
- (LLM) People can swim. Water is passable at a price, so a river is crossed
  only when walking round it would be much further; a swimmer is slower, wears
  no path, and is drawn cut off at the surface.
- (LLM) The sampling grid reads a box at eight heights rather than as one ramp,
  so the top of a box draws the top of the object and never its foot. The panel
  strip shows a row per height.
- (LLM) Sheets download as PNG and are kept in a store outside the project that
  Reset all leaves standing; kept sheets restore, or delete after confirming.
- (LLM) The sprite editor is a mode of its own beside the plant lab and the
  settlement, with the sheet drawn on the stage rather than in the panel.
- (LLM) Undo and redo cover every settable thing: a step is a snapshot of the
  whole project, recorded in the eight panel field helpers every panel builds
  on, and a control held through a range of values is one step. The three
  shading preview knobs are deliberately not recorded.
- (LLM) Images dropped straight onto a sheet, nudging art within a frame or
  across a whole sheet, and a clip that says which sheet it came from with a
  button to take that sheet again.
- (LLM) The first list: the sprite editor with layers and a color wheel,
  animation import from the editor, the import sizing fix, mirrored sprites,
  Reset all, 200x speed, relative text scaling, pinch zoom, the collapsible
  menu, and the plant lab color sampling.

## Fixed

- (LLM) The color wheel losing its disc: it kept an `ImageData`, which in wasm
  is a view onto memory that had since been reused.
- (LLM) Tree shadows reaching into the sky.
- (LLM) Settlers drawing over plants they were standing behind.
- (LLM) Town names showing whatever the Labels switch said.
