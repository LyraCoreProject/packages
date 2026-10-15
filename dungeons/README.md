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

Door and encounter rows persist in the Module across a Standalone restart. A restart preserves
open doors for completed encounters and closed doors for unfinished encounters.

## Verification

Install this Package in the Core checkout, then run its durable test:

```sh
python3 dungeons/testsuite/run.py /path/to/LyraCore test --locked \
  --test deadmines_doors -- --ignored --test-threads=1
```

The test uses a private persistent Standalone. It kills Rhahk'Zor and the Shredder, fires the
Defias Cannon through the production use path, and restarts the node. The Factory and Iron Clad
Doors must remain open; Sneed's and Gilnid's doors must remain closed. It then kills Sneed and
Gilnid and restarts again. All four doors and their completed encounter states must persist.
Core supplies the private fixture through `lyracore-test-support`; the runner selects Core and
keeps build output outside the Package.

The stage reducers in `src/deadmines_verify.rs` take no arguments because fixture GUIDs exceed
the CLI's safe integer range. After setup, each stage resolves its subjects in the Module, calls
the production kill, damage, or gameobject use path, and checks the durable outcome in the same
transaction.
Core's `module/tests/deadmines_package.rs` also calls these stages to check the boss deaths,
cannon, and Smite's weapon changes:

```sh
cd /path/to/LyraCore
cargo test -p lyracore-module --test deadmines_package -- --ignored --test-threads=1
```
