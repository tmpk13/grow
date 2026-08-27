What is left. What is done is in TODO_COMPLETE.md.

- Sprites
    - Seperate images for all human made things. (non proceedural) Including carried items.
      Buildings, walls, gates, stalls, boats and carried loads are all still drawn
      procedurally out of the sampling boxes. Wants a slot per thing, the way settler
      motions have one per motion, with the drawn art scaled to the box the generator
      would have filled.

- Farms that grow food. (Need employees/workers. And water. Initally from buckets. Maybe proximity to water or irrigation with bridges over it.)

- Actions are more reactive. 
    - Example people should not just build lights they should have fear in the night that outweighs the need for money. So rich people are more likely to build lights (Near their house), given the financial cost is the same for both.

# Lower priority.

- Scripting
    - Should be able to define animations for any given state of a non proceedural entity.
        - Fuzzy search/SLM search
        - Should be gated behind show all toggle/collapse.
        - (LLM) The ranking is already there: `find.rs` does the fuzzy and
          meaning matching for the menus and takes any list of entries, so this
          wants entries built from the animation states rather than a second
          matcher.



---
