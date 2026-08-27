What is left. What is done is in TODO_COMPLETE.md.

- Pixel editors
    - (LLM) Frames cannot be reordered by dragging, only stepped left and right.
    - (LLM) No marquee selection. Nudging moves a whole cel or a whole sheet;
      there is no way to move part of one.

- Sprites
    - (LLM) A sheet that has moved on since a motion took it gives no sign of
      it. The card offers to take it again either way, but nothing says whether
      it needs taking again.
    - Seperate images for all human made things. (non proceedural) Including carried items.
      Buildings, walls, gates, stalls, boats and carried loads are all still drawn
      procedurally out of the sampling boxes. Wants a slot per thing, the way settler
      motions have one per motion, with the drawn art scaled to the box the generator
      would have filled.

- Undo
    - (LLM) Anything added to `app.ui` rather than to the project must not go
      through the `app_*` helpers, or it puts a step on the stack that restores
      nothing. Worth a check that catches it rather than a rule to remember.

- Sprite editor
    - Should have keybinds and list them (if on desktop) example: Pick (P).

- Plants should not just disapear when they die. Should shrivel away at a set speed.
    - Default somewhat fast shrivel.

- If everyone dies the sim restarts after a settable period default 30s. Can toggle off. Off by default.

- Settings that restart the sim automatically should be changes to have a apply button. A * next to un-applied settings.
    - When switching menus without applying have a confirmation with 3 options.

- Labels toggle should not be on top bar. It should be in left menu, and have toggle all, and per category turn off/on. Make sure walls has it's own category.
    - Occupancy and grid should also be moved to the left menu (What is the left menu in desktop resolutions it joins the top in smaller screens).

- The move people toggle should be a toggle button not a checkbox next to text.

- Farms that grow food. (Need employees/workers. And water. Initally from buckets. Maybe proximity to water or irrigation with bridges over it.)

- Actions are more reactive. 
    - Example people should not just build lights they should have fear in the night that outweighs the need for money. So rich people are more likely to build lights (Near their house), given the financial cost is the same for both.

# Lower priority.

- Scripting
    - Should be able to define animations for any given state of a non proceedural entity.
        - Fuzzy search/SLM search
        - Should be gated behind show all toggle/collapse.



---
