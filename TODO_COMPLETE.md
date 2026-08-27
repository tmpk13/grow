# Done

Newest first. One line each; the reasoning lives in ARCHITECTURE.md and the
README.

- Lamps are raised by people rather than by plans. Walking home after dark with
  no lamp in sight wears on a settler and daylight, a roof and a lit street
  settle it again; a settler frightened enough, and with the coin for it, pays
  for a lamp outside where they sleep. The price is the same for everybody, so
  it is the well off who light their street, and the fear the lamp was raised
  against comes down for everybody who passes under it.
- Farms need water as well as workers. Working a field dries it out; damp
  ground near a river or lake fills it back up, and a farm out in the dry sends
  one worker at a time to the nearest bank with a bucket. A parched field is
  poor rather than barren, and the farming rate went up to match, so siting a
  farm by the water is worth doing.
- Separate images for everything people make: a slot per catalog entry, plus
  the boat and one per resource carried, grouped and folded in the Build panel.
  A picture is scaled to the box the generator would have filled, so art and
  generated things stand together; a building still going up keeps the
  generated drawing, since one image cannot say how far a wall has got.
- Marquee selection in the sprite editor: a fifth tool that drags a rectangle
  out on the stage. The nudges and Clear act on what is inside it rather than
  on the whole cel, and what leaves the rectangle is dropped, so a selection is
  a window on the cel rather than something that smears across it.
- Foliage over a settler can be solid (what a plant is), hatched, or mixed over
  them by a settable amount, so somebody walking through a wood stays findable.
  The settler is marked in the alpha byte of the composite buffer rather than in
  a mask beside it.
- Downloads from the sprite editor: the frame showing on its own, or every
  ticked sheet in one zip, a folder per sheet and a file per frame beside the
  strip when asked for. The archive is written here, stored rather than
  compressed since a PNG is deflated already, and carries no clock so two
  exports of the same sheets are the same bytes.
- Frames and layers reorder by dragging. A dragged one walks past the others
  rather than swapping with what it landed on, stays selected, and is one undo
  step; the Left and Right buttons keep the swap, which is what one press means.
- A motion says whether the sheet it took has been drawn on since. The sheet is
  fingerprinted when it is taken, so the card reads "which has been drawn on
  since - take it again to catch up" and the editor's own buttons read taken or
  out of date. Drawing something and undoing it leaves the clip current.
- Settings the running world is built from wait for Apply rather than rebuilding
  under the slider. Each one is starred, a bar says how many are waiting, and
  Discard puts them back. Leaving the panel with one waiting asks, with the
  three things it could mean: apply and go, discard and go, stay here.
- (LLM) A check that catches an undoable control that only changes `app.ui`. It
  reads the panels, finds every `app_*` call, and fails on a closure that
  records a step and then writes nowhere the snapshot could put back.
- Sprite editor keybinds: B, E, G, P for the tools, X mirror, O onion, comma
  and full stop for frames, brackets for layers. The key is on the tool button
  itself and the rest are in a folded Keys list, both only where there is a
  keyboard to press them with.
- Plants shrivel rather than vanishing. Past its age a plant browns toward
  straw and comes apart from the tips down over a settable Shrivel (s),
  defaulting to six seconds, and is only taken off the map once there is
  nothing left. Re-drawn a dozen times over the whole death, not once a frame.
- A settlement can start itself over once nobody is left alive, after a settable
  wait in settlement time. Off by default, because a town dying out is usually
  the thing being watched.
- View menu: Grid, Occupancy and the label switches left the top bar for a
  fold-out block in the side panel, reachable from any tab of the mode. Labels
  are one switch per building category with walls and gates on their own and an
  All that sets the lot; the master switch clears the map without forgetting
  what was showing.
- Move people is a toggle button that stays pressed, not a checkbox beside a
  word.
- Menu search ranks what is on the screen first: a match in the mode you are in
  beats the same match elsewhere, and one on the tab you are on beats the rest
  of the mode. The nudge is small enough that it can only settle a tie, never
  lift a weak match over a better one somewhere else.
- Menu search: a box in the top bar, `/` from anywhere, that ranks every
  control in every panel of every mode and takes you to the one you pick,
  switching mode and tab, scrolling to it and flashing it. Fuzzy by default,
  with a Meaning switch that also matches on what a setting is for. The index
  is harvested out of the running page, and the meaning table is built ahead of
  time by an embedding model that stays a build tool.
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
