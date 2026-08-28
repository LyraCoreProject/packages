# dungeons

One Package for scripted dungeon choreography, one submodule per dungeon. The whole Package
installs, enables, and disables as one unit. Deadmines is its first dungeon.

## Boundary

In this Package:

- Encounter choreography built on the kernel's notify hooks (`game_hook!`) and primitives
  (`open_door`, `spawn_wave`, `equip_swap`, `move_to_point`, HP-threshold watches).
- Speech that upstream kept in script text rather than game data (Mr. Smite's yells).
- The `debug_reducers` verification harness (`src/deadmines_verify.rs`): the stage reducers the
  durable test calls. It compiles only under that feature.

Not in this Package:

- The encounter kernel itself (`module/src/encounter.rs`): state, hooks, and primitives stay core.
- Base dungeon data: creatures, spawns, spells, and EventAI rows come from the importer. Behavior
  the vanilla data already drives (VanCleef's yells and his 50%-HP allies summon) needs no
  choreography here.
- Playerbots: bot behavior is its own Package.

To add a dungeon: create `src/<dungeon>.rs`, add its `mod` and `pub use <dungeon>::*;` pair to
`src/mod.rs`, and build its scenes on the kernel hooks. The build collapses every file here to one
module, so keep item names dungeon-prefixed.

## Deadmines

- Rhahk'Zor, Sneed, and Gilnid each open their door on death; Sneed's Shredder ejects Sneed.
- Firing the Defias Cannon breaches the Iron Clad Door.
- Mr. Smite rearms at his chest at 66% and 33% health: dual Reavers, then his two-hand hammer.

Entries and positions are dump-verified against classic-db z2815. Omitted: Smite's stomp stun (no
vanilla data row backs it).
