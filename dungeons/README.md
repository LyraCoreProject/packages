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
  If a completed encounter's door is recreated closed, the next creature tick opens it from saved
  completion. A reset, unfinished encounter, or another instance keeps its own door state.
- Firing the Defias Cannon breaches the Iron Clad Door.
- Mr. Smite rearms at his chest at 66% and 33% health: dual Reavers, then his two-hand hammer.

Entries and positions are dump-verified against classic-db z2815. Omitted: Smite's stomp stun (no
vanilla data row backs it).

The restored boss doors follow the load behavior in
[CMaNGOS's pinned instance script](https://github.com/cmangos/mangos-classic/blob/ca0775fe352fb67a0e82dc6051f0563187d569a5/src/game/AI/ScriptDevAI/scripts/eastern_kingdoms/deadmines/instance_deadmines.cpp#L81-L98),
also represented by
[OMaNGOS's pinned Deadmines script](https://tangled.org/carl.tngl.sh/OMaNGOS/blob/d3f8c589aeb06d8a878050a4a11256479011a031/crates/content/src/eastern_kingdoms/deadmines.rs).
The Package checks live gameobjects indexed to map 36. Core has no gameobject-created hook, so
the Package uses its existing tick marker and calls `open_door` only for a closed boss door whose
own encounter is Done. It does not replay the boss death.

## Verification

Install this Package in the Core checkout, then run its durable test:

```sh
python3 dungeons/testsuite/run.py /path/to/LyraCore test --locked \
  --test deadmines_doors -- --ignored --test-threads=1
```

The test uses a private persistent Standalone. It completes all three boss encounters through the
kill path, deletes their door rows, restarts the node, and recreates closed doors twice. Each door
must reopen without another boss death. Closed copies in reset, InProgress, and Failed encounters,
another map, and instance 0 must stay closed. Core supplies the private fixture through
`lyracore-test-support`; the runner selects Core and keeps build output outside the Package.
