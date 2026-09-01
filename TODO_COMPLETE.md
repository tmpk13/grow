# Done

Newest first. One line each; the reasoning lives in ARCHITECTURE.md and the
README.

- (LLM) A midground: hills, ridges and mountains standing in the sky band
  behind the map. With Place on and the placing menu holding scenery, a press
  on the sky puts one up and the same gesture drags it - sideways to move it,
  up and down for its size - and the Land panel lists what is standing, what
  each piece is made of, and takes them down again. How far off a piece is
  hazes it into the sky and decides what stands in front of what, so nothing
  has to be shuffled by hand. It is drawn into the cached ground over the sky
  and under the land, so the map covers its foot, and it is scenery only:
  nobody walks on one and the town never asks about it.
- (LLM) Somebody who ends up a long way from their own town and stays there
  gives up on walking back and founds one where they stand, with anyone else
  out there beside them. They bring what they know and nothing else: no stores
  and no storehouse, because nobody planned it. The patience is measured in
  seconds rather than days, since a person covers the width of a small map in
  under a minute and anything longer would only ever be spent walking home.
- (LLM) Zones and ground drawn from a picture: drop one on the Land panel and
  it is laid over the whole map corner to corner, press it to take the color
  under the pointer and drag out the piece to work on, and every cell in that
  box near enough to that color becomes water, rock, grass or sand, or is zoned
  for what may take root there - nothing, trees only, or the low growth only.
  Zones hold for whatever seeds next rather than pulling up what is already
  there, a hand planting something outranks them, and both the drawn ground and
  the zones are written down with the settlement, which is otherwise made again
  from its seed.
- (LLM) A placing menu: with Place on above the map, every press puts down what
  the menu on the Build panel is holding - a building from the catalog, laid as
  a site or put up finished for the chosen town, a plant of any species the
  project holds, or a load of anything. Everything placed is placed the way the
  town would have placed it, so it is built, harvested and hauled by the same
  rules; a press that cannot put it there says why rather than moving the map
  out from under the aim.
- (LLM) A person can be taken over, under the experiments switch: a Take over
  press hands them to the keys or to a stick on the map, and the row under it
  is the four things they can be asked to do - cut what is in front of them,
  pick a load up or put it down, step in or out of a doorway, eat what they
  have. They plan nothing for themselves until they are let go, and everything
  else about being a person still happens to them, so a driven person starves
  if nobody feeds them. Water is swum rather than walked round, and a wall stops
  only the part of the push that is into it.
- (LLM) Anything the town has put up can be condemned from its Look inside
  card, and the people take it apart themselves: the same walk and the same
  effort as building, measured against what it cost to raise, with most of the
  materials left on the ground where it stood. A condemned thing empties first
  - beds, benches, deeds and counters - and is counted out of the town while it
  comes down, so the planner starts on what replaces it straight away; letting
  it stand again puts it right at the cost of the work spent. A site that has
  not gone up is called off instead, and everything carried to it is left where
  it stood.
- (LLM) Nobody is cut in half by water any more. The generated person has
  poses of its own for crossing water and for treading it - head and shoulders
  over a waterline, an arm out with the stroke, a bob while treading - and two
  new slots, Still in water and Picked up, take art for them. Only a pose
  borrowed from dry land is still cut at the waterline, which is what puts it
  in the water rather than on it. Somebody lifted by Move people hangs off the
  ground with their arms up and their feet swinging.
- (LLM) Art carries its own size. One number, art pixels per cell, says what a
  source pixel is worth against a map cell, and both sides of a frame go
  through it together, so nothing dropped on a slot is ever squashed to fit a
  box the generator would have drawn. Frames keep the padding they were drawn
  with, the editor canvas and the import cap go to 256 pixels, pixels are saved
  run length encoded, and a picture dropped on a smaller frame grows the frame
  rather than shrinking the picture.
- (LLM) Clouds passing over the settlement, settable: cover, drift, and how
  strongly the edges churn as they pass - the shapes boil slowly rather than
  sliding as one picture. One seamless tile of wrapped value noise, on
  simulation time like the wind, stamped over the sky band under everything
  that stands up into it and fading into the horizon; a further switch makes
  the empty space around the map the same sky, gradient, clouds, night tint
  and all.
- (LLM) A Look inside switch, the fourth exclusive press: a pressed building
  lands its card on the Build panel - state, deed, household, rooms, crew,
  bench, and who is under the roof this moment, which the map itself cannot
  show. The card stays until dismissed, and open ground still moves the map.
- (LLM) An Add people switch beside Move people and Harvest, exclusive with
  both: each press sets a new person down where it lands. They arrive grown
  with a founder's purse, join the nearest town still standing, land the way a
  put-down does - water included, and a blocked cell hands them the nearest
  one somebody can stand in - and plan for themselves from there. The arrival
  goes on their record and into the town's book.
- (LLM) Wind in the trees, settable: standing plants lean from the tips by
  their own height, each in its own phase, with a gust that travels across the
  map. It runs on simulation time, so a paused world holds still and the same
  seed is still the same picture; the lab, which composites only when dirty,
  sits it out, and it goes with the other flourishes when the camera pulls
  back.
- (LLM) A dying tree's shadow follows what is left of it. The contact shadow
  was sized by the canopy radius, which only ever grows; it now takes the
  drawn box where that is smaller, so a crown eaten from the tips down stops
  shading ground it no longer covers, and a plant with nothing drawn casts
  nothing.
- (LLM) The side menu has a drag handle on its edge: the width is kept in rem
  so it rides the text scale, it is saved with the window preferences, and a
  double press puts the stylesheet's own width back.
- (LLM) The chrome moves instead of blinking: folding the menu away slides its
  track shut and fades it, sections ease open and closed, the view dropdown
  fades, and every ease honors prefers-reduced-motion. All of it is CSS over
  state that still changes instantly, so nothing waits on an animation.
- (LLM) Lamp pools are screened together rather than summed, so a street of
  lamps converges on the color of one flame instead of stacking past it to
  white.
- (LLM) The View menu is a dropdown in the top bar rather than a block of the
  side panel, folds shut on a press anywhere else, and is gone entirely in the
  sprite editor, which draws none of its overlays.
- (LLM) Every section of every panel folds by its own head, a Fold all button
  over the panel pulls them all one way and then offers the way back, and the
  folds are window preferences, so they survive both the constant panel
  rebuilds and a reload. Menu search unfolds whatever it lands in.
- (LLM) A row of chips says what it is: a small head over the towns, the
  trades, the register order, a person's temperament, the sheets and the
  research effects.
- (LLM) An optimization pass over the settlement tick, byte-identical to the
  pixel: a 200 day run in half the time, a phase timing harness kept for the
  next pass (`GROW_PHASES`), and the plant raster - most of the bill, and
  invisible to the earlier hand timings - confined to what a plant actually
  drew.
- (LLM) An Experimental tab with one switch over it, off by default. Nothing
  under it is asked anything while it is off, so the settlement is the one it
  has always been, and turning it off mid flight puts the world back.
- (LLM) Hot air balloons, the first thing in that tab. A town with a school and
  cloth to spare sews a canopy, burns charcoal under it and sends it up over
  itself; research runs faster for as long as one is in the air. It climbs,
  drifts on whatever wind it caught and comes down, and the panel lists what is
  up and where.
- (LLM) The map can be made larger under a running settlement. The new land goes
  on the right and along the bottom, so nothing standing on the old map moves;
  it arrives with a wilderness on it, warmed for as long as a fresh map is and
  with the old land held still while that runs. Rivers and deposits are placed
  in the new land only, and a course traced into the old map stops at the
  boundary rather than cutting a channel through somebody's town.
- (LLM) How far somebody will walk for a load on the ground is a setting, and
  half again as far for one cut by hand. Past it they leave it - unless there is
  nothing nearer to fetch at all, in which case the nearest one beyond reach
  goes in below every other job there is.
- (LLM) A cut plant goes over rather than vanishing out of the hand that cut
  it: anything with a stem tips from its foot over a settable second or so and
  is only off the map once it is down. The ground it stood on is given back at
  once, so something else can start growing under it.
- (LLM) People walk round trees. A shrub, tree or vine with enough of itself
  standing in a cell shuts that one cell to the pathfinder, the trunk only, so
  a wood is walked through rather than round. Settable, and the grid is saved
  with the settlement because it is worked out on a timer and a restored town
  would otherwise walk round a tree it had not noticed yet.
- (LLM) A house nobody lives in falls in. Past a settable wait it loses its
  roof from the ridge out and then its walls from the top down, weathering as
  it goes; when it is gone the ground comes back and a quarter of what it was
  built from is left lying in the rubble. Somebody moving in at any point puts
  it right again at the same rate.
- (LLM) The tool opens on the settlement rather than the plant lab, and with
  nobody touching anything for twenty seconds the map takes the whole window on
  its own. That is the page folding its own chrome away rather than the browser
  going fullscreen - an untouched window cannot ask for the screen - and a
  moved pointer, a key or a touch hands the menus straight back. Settable, zero
  never does it.
- (LLM) Anything left on the ground rots over a week rather than four days, and
  the setting says so.
- (LLM) A saved settlement comes back bit for bit. serde_json's default float
  parser is not correctly rounded, so a number written exactly came back a
  fraction out and a fortnight later it was a different town.
- (LLM) A Harvest switch over the map: hold or drag over what is growing and it
  comes down, with a bar per plant that fills while the pointer is on it and
  runs back out if it is let go of too soon. What a cut is worth is left where
  the plant stood and marked as asked for, which puts it in front of the work a
  person would otherwise have chosen - a town short of food excepted. Every cut
  is remembered against its species, and the gatherers walk further for a
  species that has been cut for them and take it smaller than they would have
  bothered with; what has been taught is listed in the Tech panel. Everything
  cuttable in view pulses faintly, and whatever is under the pointer pulses
  firmly.
- (LLM) Every checkbox is a toggle button: square and pressed in with a check
  drawn in it for the settings rows, the word on it for the switches in the
  toolbar and beside the search box. There is no checkbox left in the program.
- (LLM) Text size is applied when the slider is let go rather than while it is
  dragged, since what it resizes is the page the slider is sitting in, and
  there is a number box beside it that takes a size in per cent.
- (LLM) A running settlement survives a reload. It is written to a store of its
  own every twenty seconds and again when the page is closed or hidden; coming
  back picks the same town up on the same day rather than founding a new one.
  Only what could not be worked out again is saved: terrain, walkability, layer
  occupancy and every cached picture are rebuilt from the seed and from what is
  saved. A file names the world it grew on and is refused rather than half
  applied if the map has changed since. Down to the pixel and through the next
  hundred days, the restored settlement is the one that was saved.
- (LLM) The build number, beside the name in the top bar and stamped into every
  exported project, with a test that the crate and the package agree on it.
- (LLM) The Meaning switch is a toggle button grouped with the search box it
  belongs to, rather than a checkbox adrift in the row of project buttons.
- (LLM) The View menu's arrow is a triangle cut out of a square rather than a
  glyph, so it sits on the middle of the line and stays there when it turns.
- (LLM) The eight clippy warnings that had been standing in `civ_render`,
  `tasks` and `plant`: two identical branches of an if, four `x - 1 >= y`
  comparisons, a collapsible match arm and a hand written checked division.
- A picture per state of every made thing: Always, Going up, With somebody at
  it, After dark, and Carrying cargo for a boat. A thing with a picture for one
  state only is drawn from it in that state and generated the rest of the time.
  The hundred and thirty slots are searched rather than listed, using the same
  ranking the menus use over an index and a meaning table of their own, behind
  an Every slot switch.
- Lamps are raised by people rather than by plans. Walking home after dark with
  no lamp in sight wears on a person and daylight, a roof and a lit street
  settle it again; a person frightened enough, and with the coin for it, pays
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
- Foliage over a person can be solid (what a plant is), hatched, or mixed over
  them by a settable amount, so somebody walking through a wood stays findable.
  The person is marked in the alpha byte of the composite buffer rather than in
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
- (LLM) Move people: a switch above the map turns a press on a person into
  picking them up. The pointer carries them, letting go puts them down on the
  nearest ground they can stand in, what they were doing is given up the way any
  change of plan gives it up, and the tick leaves them out while they are held.
- (LLM) Lamp posts, built like anything else, lighting a pool of ground after
  dark by drawing over the night tint rather than cutting holes in it.
- (LLM) The fullscreen escape hatch fades out when nothing has moved for a
  couple of seconds.
- (LLM) People with no bed walk back to town before lying down, and turning in
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
- (LLM) People drawing over plants they were standing behind.
- (LLM) Town names showing whatever the Labels switch said.
