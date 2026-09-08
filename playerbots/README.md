# playerbots

A standing population of session-less Characters, so a small realm still has a party to test content
with.

A bot is a real Character on a Package-minted Account: a `game_character` row, a live
`game_world_entity` row with the PLAYER type mask, a spellbook, and durable position. What a bot does
not have is a Session — nothing calls `player_login` for it. That one difference is the design. The
bot is durable, so it survives a Gateway restart and a republish. The bot has no Session, so the
Gateway's session-less paths already treat it correctly: a player can invite it by name, and it
refuses whispers.

## Operator surface

Every verb is Operator-gated. Call them with `spacetime call <database> -- <verb> <args>`.

| verb | what it does |
| --- | --- |
| `playerbots_spawn N x y z` | Spawn `N` bots at a point, cycling the roles so the batch forms a party. |
| `playerbots_spawn_role N x y z role` | Spawn `N` bots of one role, using this Package's default class for it. |
| `playerbots_spawn_class_role N x y z class role` | Spawn `N` bots of one class and role. Refuses a pairing this Package has no kit for. |
| `playerbots_populate` | Top the population up to `population_count`. Idempotent. |
| `playerbots_despawn_all` | Delete every bot Character and everything it owns. |

Roles are `0` tank, `1` healer, `2` damage.

Nothing populates on its own. A realm gets bots when its Operator asks for them.

## Package Config

The Package seeds these keys on each Shard the first time its code runs there, and never overwrites
a value an Operator has edited. Read them with
`spacetime sql <database> "select * from game_package_config"`, change one with
`set_package_config`.

| key | default | meaning |
| --- | --- | --- |
| `population_count` | `10` | What `playerbots_populate` tops up to. |
| `home_x`, `home_y`, `home_z` | Elwynn Forest | Where a populated bot spawns, and where an ungrouped bot wanders. |
| `level_min`, `level_max` | `1`, `10` | The level band a spawned bot is created at. |

A Shard that has never run this Package's code shows no rows for it. That is the signal that the
Shard has not seeded yet, not a fault.

## Behaviour is data

`pkg_playerbots_kit` says what a `(class, role)` bot learns. `pkg_playerbots_rotation` says what it
casts, in priority order, and under what condition. Both are rows. A SQL `UPDATE` on a rotation row
changes how a bot fights while the realm is up, with no republish. The Paladin is the worked example:
one class, three roles, three different rotations, no code that knows the difference.

A `(class, role)` pair with no kit rows is a pairing this Package cannot fill, and the spawn verb
refuses it by name. Adding kit rows for a new pairing makes it legal.

`pkg_playerbots_personality` holds the part of a fight the rotation leaves open: where a bot breaks
off, and where a healer places a heal. Two bots on one rotation at the same health diverge on those
alone. The row is the floor; a Runtime Script can answer for either of them instead.

## Personality as a script

A row is one number. A script is a decision. This Package exposes both personality axes as Package
Events, so a Runtime Script it ships can answer them per bot, per fight, and an Operator can change
that answer while the realm is up.

| event | `event.actor` | `event.target` | the answer |
| --- | --- | --- | --- |
| `playerbots.flee_at` | the bot | what it is swinging at, or nil | the share of maximum health it breaks off at, 0 to 100 |
| `playerbots.heal_at` | the healer | the ally most in need of a heal | the share it heals an ally at, 0 to 100 |

**The row is the fallback, always.** A number in `0..=100` is used. Everything else is the
personality row: no script bound, a script switched off, a script that failed on syntax, one that
raised, one that ran out of Fuel, one that returned nothing, and one that returned a number that is
not a share. Out of range is refused rather than clamped — a script answering 5000 has a bug, and
clamping that to 100 would make every bot flee at full health while the log said nothing. A
fractional answer truncates, because a share is a whole percent everywhere else here.

**A heal answer can only ever tighten.** The effective threshold is the lower of the rotation row's
own share and the answer, so a script can make a healer wait longer but never out-heal its rotation.
That is the rule the personality row already had, unchanged.

**What it costs.** One `ask` per bot per think for the flee share, which is once a second; one more
for the heal share, and only for a bot whose rotation actually has a heal to place. An event nothing
is bound to is one indexed range scan and nothing else.

**What ships.** `data/.generated/personality.json`, a Script Artifact holding two scripts:

- `playerbots.cautious-flee` — a low-level bot has nothing to spend and everything to lose, so it
  bolts at 39% while a level 30 one holds to 10%. A bot already under half health leaves a little
  earlier than it would have.
- `playerbots.steady-heal` — the member with a bigger health pool than the healer's is the one
  taking the hits, so the rotation row's own share stands for it. Everybody else waits until they
  are properly hurt.

The file is **hand-written**, not generated. Its `source_hash` is 64 zeros for that reason: nothing
upstream produced it, so there is no Datascript revision to record. LyraCore#320 (TypeScript to Lua)
would generate the same artifact through the same runtime path; until it exists, hand-written Lua is
the supported way to ship a Runtime Script.

**Reconciling an edit.** Editing the file changes nothing by itself. Apply it to every Shard:

```bash
spacetime call <database> apply_package_deltas '"script"' "$(
  for f in packages/*/data/.generated/*.json; do jq -c 'select(.kind == "script")' "$f"; done | jq -Rs .
)"
```

Two things about that command are load-bearing. The payload is the WHOLE enabled plan, every
Package's Script Artifact and not just this one: an apply clears the Package script range and
rewrites it, so naming one Package would delete every other Package's scripts. And each artifact
travels on ONE line, which is what `jq -c` is for — the file here is pretty-printed because a human
edits it.

No republish is involved, and none is needed. LyraCore#393 tracks carrying the `script` family
through `lyracore packages replay`, which would make this one command for the whole realm instead of
this.

**What a script cannot do.** Nothing new. A Runtime Script sees the curated verb surface the Host
already offers — `heal`, `send_chat`, `grant_xp`, and the snapshotted Entity Handle fields — and no
query surface. These two events add a return value the Package reads; they add no verb.

## The mind

One `game_tick_pass!`, with each bot throttled to a decision a second. There is no Package-owned
schedule row, so a republish cannot leave the bots pointing at a reducer the new wasm no longer has.

Each decision, in order: put a body back on if the bot has none; get back up if dead; break off if
hurt past the personality threshold, as a Runtime Script or the personality row settled it; follow a leader who has crossed into another map or instance of
this Shard; cross a Shard boundary when the party is not on this one at all; quest, unless a player leads the
party; fight what is on the party; otherwise follow the leader, or wander near home.

`pkg_playerbots_goal` holds what the bot settled on and when it settled on it. Read it to see what a
party is doing:

| kind | meaning |
| --- | --- |
| `0` | following the leader |
| `1` | fighting |
| `2` | broken off, running home |
| `3` | wandering near home, ungrouped |
| `4` | off its home ground with no party on this Shard, waiting |
| `5` | a Transfer Intent is out; the bot is crossing |
| `6` | running to a quest giver, back to the one that ends a quest, or back to its home ground |
| `7` | working a quest objective |
| `8` | no quest work available; killing for experience |
| `9` | dead; releasing to the graveyard and resurrecting there |

The same row carries `stalled_since_micros`. It is `0` while the bot is getting on with things and
the wall-clock time it got stuck otherwise:

```sql
select character_guid, kind, stalled_since_micros, stall_warned from pkg_playerbots_goal
where stalled_since_micros <> 0;
```

It is a separate column because `since_micros` cannot answer the question. That one restarts every
time the goal CHANGES, so a bot flapping between walking back for a turn-in and grinding looks brand
new on every tick you read it, however long it has been getting nowhere.

Three things stop the clock, and they are outcomes rather than goals: a quest accepted, a quest
turned in, and a swing at something a held quest names. A walk is not one of them. Reading the goal
kind instead was the first version of this and it under-reported the case the clock was added for —
the speculative walk back to the quest hub records `6`, so a bot flapping between the hub and its
grinding ground cleared its own evidence on every excursion. Joining a PLAYER's party stops the clock
as well: a bot following a player does no quest work, so nothing there could ever clear it.

A bot that has been stalled for a minute says so once in the log, with what to read next, and
`stall_warned` is the latch that makes it once. A window on the clock cannot: any gap in the think —
scheduler jitter, a republish, a tick spent walking back inside the leash — steps over the window
and the warning is lost while the clock runs on.

Every action leaves through a core operation the player path also uses — the actor verbs for attack,
stop, cast, invite-accept, quest accept and turn-in, loot, release and resurrect, and the shared
creature leg writer for movement. The Package decides what to do; the core decides whether it is
allowed.

## Questing

An ungrouped bot works quests around its home point: take one, kill what it names, take what the
kill leaves, hand it back. A bot with nothing to take and nothing to work kills for experience
instead. Both are visible in the goal table.

One rule decides whether a quest is worth walking to, and it is the core's own rule rather than a
copy of it. The Package calls `crate::quest::accept_gates`, the same function `apply_accept_quest`
applies before it opens a quest-log row: level, race, class, the previous step in the chain, and
whether the bot already holds the quest, in that order. The core accept is still what answers for
real. Picking any other way is how a bot ends up running to a giver, being refused for a
prerequisite it has never done, and running there again the next second, which is what the July
foundation did on an imported node.

The Package used to carry a copy of those Gates plus a copy of the core's completeness test, and
four source scanners to hold the copies in place. The core made both seams callable, so the copies
and the scanners are gone. Two questions are all that is left:

- One Gate the core's own does not answer, because the core reaches it last: a quest that hands an
  item over on accept needs a free bag slot, and the accept grants that item from inside its effects
  after every Gate has passed. The Package asks it after `accept_gates`, so the order still matches.
- Whether the turn-in would be accepted, which is `crate::quest::quest_is_complete` plus the
  deadline. That function answers objectives only and never reads the deadline, so the deadline is
  asked here, first — between a timed quest running out and the sweep that marks it failed, the row
  still reads active.

A bot never abandons a quest. The log row is the only memory it has that it already chose one, and
dropping the row is what lets the loop back in — so a quest a bot cannot finish holds its slot for
good. A bot works three at a time, which is what keeps one such quest from ending its career.

That is why selection asks a question of its own after the accept gate, and why that question may
only ever be stricter. A quest whose objectives are all "use this gameobject" or "explore this
place" can never be finished by a bot: both are credited from a message a client sends, and a bot
has no client. It is not taken. Neither is a quest that carries an event requirement, whatever its
objectives say: that credit is written by an EventAI action on a creature the objective rows never
name, so the bot has nothing to aim at and the turn-in would refuse forever. A quest with no
objectives at all IS taken, because that is the talk-to quest that opens most chains, complete the
moment it is accepted. A quest that mixes something the bot can do with something it cannot is taken
too: part of it is worth watching.

Where a quest was taken is remembered on the goal row, because a bot ranges further than it can see
and the giver is usually out of sight by the time the work is done. Without that bookmark a quest
taken at the edge of a bot's patch could never be handed back.

A bot that ends up holding quests it can make no progress on says so, in the log after a minute and
in `pkg_playerbots_goal.stalled_since_micros` from the first tick. It also stops walking back on
spec once the clock has run, so a stuck quest costs a slot rather than a leg a second.

A bot takes coin and wanted quest items only from a creature corpse eligible to its Character. The
Module's death entitlement gives quest credit to eligible Characters, regardless of which unit lands
the lethal hit. It cannot sell and it cannot destroy, so anything else it picked up it would keep for
the rest of its life, filling the bag that taking a quest needs room in, for copper it can never
realise. Leaving the trash on the corpse is what keeps the bag usable, and it is why the bot never
has to reserve a slot against its own looting.

A quest that hands an item over on accept is not chosen when the bag is full, because the core
refuses it there. Handing back is split. A quest with collected items to give back is always walked
to, because the turn-in removes them before it grants the reward, exactly so a full bag can still
finish a collect quest — and that is the only thing that ever gives a bot a slot back, so refusing
to set off would shut it. A quest with nothing to give back has nothing to free, so on a full bag it
is not walked to at all: that trip could only end in a Refusal.

## Serendipity

The reason to run this Package: you are killing kobolds, somebody on the same quest asks you to
group up, and you do.

An ungrouped bot with quest work in its log looks around about every fifteen seconds, staggered by
Character so two bots on one pad never look on the same second. It invites the first fellow quester
it finds within forty yards: another Character, alive, on its own team, in no party, and holding one
of the quests the bot is still working. The shared quest is the whole rule — the invite means "we
are both killing these", so without one there is nothing to say. A bot target answers for itself; a
player gets the ordinary invite dialog from their own client.

The Package decides and the Gateway executes. Party membership is authoritative on the realm's own
directory database, which a Package can never reach, so the bot writes one intent row and stops —
the same split a Shard crossing uses. Everything that could refuse the invite is the core's answer,
on the correct authority.

A bot-led party quests. Every bot in it works its own log on its own leash, and only the fight is
shared. Without that, two bots that found each other would both stop questing, and a population that
had all paired off would never invite anybody again.

The party ends with the work. On each think, a bot that LEADS its party and shares no un-rewarded
quest with anybody else in it leaves. Leadership passes by the core's rule and a party of one
disbands, so a player who was invited sees the party end when the two of you are done. That leave
takes the same relay the invite does, for the same reason.

## Death

Death is part of the loop, for every bot, in a party or not. A dead bot releases to the graveyard
and resurrects there, one step per tick. The quest log survives both, so the bot resumes the quest
it died on rather than choosing again.

Releasing can move the bot to another map, because the graveyard a death in a dungeon resolves to is
outside it, and a cross-map placement takes the bot's live entity with it. The Character row
remembers that the bot was a ghost, and the tick rebuilds it as one. Without that the bot would come
back alive on the spot, with no resurrection, no sickness and its corpse left behind.

## Crossing a Shard boundary

A party that walks into a dungeon on a sharded realm crosses to the Shard that serves it. The bots
follow, and they come home again afterwards, through the same Transfer a player uses.

Both directions read one rule off this Shard's own rows, because a Package never gets a directory
of where anybody is:

- **In.** The leader has no entity here, and the party has a live instance here. That instance row
  is what resolved the leader's portal, and the Shard they set out from keeps it. The bot is placed
  at the portal's landing point and one Transfer Intent is written.
- **Home.** The leader has no entity here, and there is no party instance here either — which is
  what the Shard that serves the dungeon looks like from inside one. After ten seconds the bot
  crosses back to its home point.

The ten seconds are the difference between being abandoned and having simply arrived first: bots are
driven across one at a time, so a bot can land a moment before the leader it followed.

Arriving is not a special case. A Transfer carries the Character row, the roster row and the
personality; it does not carry the goal row and it does not carry a live entity. So a bot arrives
with no body and no goal, and the ordinary tick rebuilds one and decides afresh. That is the whole
of arrival.

On a realm of one Shard the same code runs and the crossing is already finished when the Intent is
written, because the placement was the whole move. The Gateway says so and the bot is back in the
world about three seconds later. The same three seconds are what recovers a bot whose crossing was
never driven at all — a republish in the middle of one, or a Gateway that was down. An Intent is a
request, not a record: nothing refuses it and nothing retries it, so the deadline is the way back.

## Limits

- Movement is a straight line. Bots do not use navigation data.
- A leader who logs out is a leader who is not on this Shard, so the bots go home after the wait.
- A party the leader has LEFT is led by whichever bot inherited it, and a bot leader is on this
  Shard by definition — so those bots never go home as a party. They dissolve it instead: the
  inherited leader shares no quest with anybody, leaves, and whoever inherits next does the same,
  until the party is gone and each bot is questing on its own again.
- A crossing is aimed at a portal into the destination map. A dungeon map with no imported portal
  row is a dungeon the bots cannot follow anybody into; they wait, then go home.
- A bot in a PLAYER-led party does not quest. It follows the leader, which is what a player
  invited it for. A bot-led party quests, and its leader disbands it once the shared work is done.
- A bot quests within 150 yards of its home point and looks 60 yards ahead. Move the home point to
  move the patch.
- The objectives a bot works are the ones it can kill, and the ones it fills by looting what it
  kills. A quest made only of the two it cannot work is never taken. A quest that mixes the two IS
  taken, and holds one of its three slots until the Operator does something about it; the stall
  column is where that shows up.
- A bot takes the first reward choice, because it has no gear plan to pick against.
- A personality script answers a share and nothing else. It cannot pick a target, cast, move a bot
  or read the world beyond the two Entity Handles the event carries, because the Runtime Script Host
  offers nothing else and this Package added no verb to it.
- Editing `personality.json` changes nothing until it is applied to every Shard. There is no watcher
  and no automatic reload; a Shard that missed the apply keeps the scripts it last got.
- A bot picks its own fights only inside its own level band: no more than three levels up, nothing
  so far down that the kill pays no experience, and never an elite. With nothing in the band in
  sight it wanders instead.
- A bot with a full backpack keeps grinding and keeps handing quests back, but stops taking quests
  that hand an item over on accept. Nothing on the Package surface sells or destroys, so the only
  thing that frees a slot is a collect quest's own turn-in.
- The bot asks a stricter question about bag space than the core does. The core tops up a partial
  stack of the same item before it needs a slot; the bot only asks whether a slot is free. It can
  therefore pass on a quest the core would have given it. That is deliberate: passing on a quest is
  harmless, taking one and being refused at the giver is the loop.
- A quest whose ender is neither in sight nor at the giver the bot took it from cannot be handed
  back. The bot makes the trip to the giver once, finds nothing, and after that its stall column
  says so.
- The stall clock cannot tell a quest creature on a long respawn from one that does not live near
  the bot at all. Both leave the bot grinding with a kill objective it cannot serve, so a healthy
  bot in a thin spawn area can warn at a minute. Read its `game_character_quest` rows before acting
  on the warning; the fix would need a respawn timetable the Package has no read of.

## Action fixture

`module/tests/playerbots_rewrite.rs` in LyraCore drives real Package requests through a private
Standalone. Install this Package into that core checkout, then run:

```sh
cargo clean -p lyracore-module
cargo test -p lyracore-module --test playerbots_rewrite -- --ignored --test-threads=1
```

The input record is `fixtures/actions.json`. Each run records the tested core and collection commits,
whether either worktree is dirty, the Package Content Identity, and the Module wasm identity under
`/tmp/lyracore-standalone-logs`. Successful cases also save action, quest, cast, and position observations there. Content comes from core seeds and reserved fixture rows. Obstructions
come from synthetic navigation cells. No imported client geometry is present, and these cases do not
prove imported-world routing or real-client appearance.

The fixture uses the existing gameplay operations. A Cast Handle correlates a scheduled spell with
its terminal outcome; accepting an attack does not certify a hit or quest credit. The bounded
`pkg_playerbots_action` table retains the latest observation of each action kind per Character.

The rewrite adopts AzerothCore's decision grammar in Rust. The reference inventory at
`b949b50bfcdd4fab937781bac2d7765e39330e4b` counts 2,967 lines for the selection kernel and 243,560 for
the whole runtime. Even the kernel retains C++ host pointers and packet-bearing events. Porting that
runtime would require synthetic WotLK World Sessions, packet queues, and host objects before replacing
them with LyraCore's Module operations. Keeping typed decisions and durable identifiers beside the
existing cast, combat, navigation, quest, and item operations is the smaller total implementation.
The counts describe the reference source, not an estimate of the Rust rewrite.

When sharing `CARGO_TARGET_DIR` between checkouts, clean the Module after changing the worktree or
installed Package inventory. Its build script discovers Package files and hooks. A shared cache can
otherwise reuse discovery output from the previous checkout. Dependency artifacts can stay cached.

Cast observations retain the original Cast Handle while waiting. Instant effects resolve in the
request itself, before any scheduled completion callback. `CastResolved` means effects dispatched;
a projectile can still be in flight. Channel requests explicitly return `UnsupportedChannel` until
channel ownership is implemented. Targeted casts currently have no shared line-of-sight Gate; this
fixture does not invent a line-of-sight refusal. Actor requests also retain the existing spellbook
bypass; later capability selection must require known, supported spells.

Movement observations retain complete, partial, blocked, and direct planning status, clipping,
coverage, and a last-advance time measured from successive actual positions. Complete planning is
separate from arrival. Attack acceptance is separate from quest credit; the quest progress clock
resets only when objective counts increase or a synchronous quest operation succeeds.

Quest interactions finish synchronously. The core plans collect-item consumption and all rewards
before changing inventory, so a caught turn-in Refusal leaves items, money, experience, and the quest
unchanged. The fixture covers an invalid reward choice, full inventory, successful retry after a
slot is freed, and a source-item acceptance refusal. Imported item uniqueness limits now apply to
storage and reward exchanges, including bank holdings and other rewards in the same exchange.

`pkg_playerbots_goal.quest_credit` is an end-appended nullable count with a null default. The first
observation establishes its baseline without clearing an existing stall. Later increases count as
progress. The fixture exercises an existing stalled goal with nonzero objective credit and this
migration default; it does not claim to publish an older Package binary before upgrading it.

## Durable controller and objective

`playerbots_select_controller(guid, controller)` selects one controller for that Character. The
SpacetimeDB argument names are `legacy`, `recordOnly`, `cohort`, and `frozen`.

| Controller | Behavior |
| --- | --- |
| Legacy | Existing quest and party policy. This is the additive migration default. |
| RecordOnly | Records the new decision without issuing gameplay from either controller. |
| Cohort | Runs the durable objective and typed action runner. |
| Frozen | Cancels Package casts and movement, stops bot attacks, and prevents new bot work. |

Selection also updates core-owned session-less action consent. Legacy and Cohort allow group
admission; RecordOnly and Frozen suppress it. Every selection clears this Character's unclaimed
Group Intents, including a repeated selection. A group action admitted before the selection may
finish at Realm-core afterwards. There is no atomic operation across Shards.

Every gameplay entry checks current Account Claim and Fence ownership and Character World Session
status. A human taking ownership suspends the bot even before `online` changes. Cancellation matches
the Package cast identity or movement start time, so it preserves a human's later work. Bot control
can resume after ownership ends. The ownership Gate permits an absent body for Legacy restoration;
group admission additionally requires a live entity.

The Cohort behavior in this revision returns to the roster's home point. It retains that objective
while healing itself with a known supported rotation spell, defending against damage, or fleeing
at its configured health threshold. Companion orders and quest catalog execution remain separate
work. The existing Legacy policy remains available under its explicit selector.

Recovery examines at most 24 matching healing rotation rows per pass. The separate
`pkg_playerbots_recovery_scan` explanation records its indexed cursor, examined row count,
and Pending or Complete stage. A first incomplete scan selects Recovery Hold. Completed
scans restart on the next pass, so inserted or edited rows are reconsidered. A retained
spell is checked against its current class, role, condition, and exact learned-spell row
before use. Defense in this controller responds to the live attacker recorded by incoming
damage; target acquisition and assistance remain downstream work.

The normal Package tick reads the `(next_think_micros, id)` index and processes at most 16 due bots.
Excess bots keep their due time. They precede bots whose turn already advanced the clock. The
scheduler row reports the processed identities and the oldest deferred lag. Runner rows backfill
only for that bounded batch, or for one explicitly selected bot.

`pkg_playerbots_runner` is the explanation read. It reports the retained objective and destination,
chosen target and reason, ordered candidates, foreground cast or movement, latest outcome, progress
age at `observed_micros`, retry count, next eligible time, and catalog revision. It retains eight
meaningful transitions, four failures and four deferred destinations per Character. Movement,
combat, cast and quest evidence have separate clocks. An accepted attack does not advance any of
them. A completed cast records dispatched effects, and does not complete the return-home objective.

One foreground action retains the controller generation and partition. A higher-priority action
cancels incompatible work before starting. Pending casts retain the core scheduled identity and
refresh its current due time after direct-damage pushback. After a movement request, arrival requires a later position
observation. Ten seconds without movement records a failure; three failed intervals defer the
same destination for 30 seconds. Deferred keys include map, instance and optional navigation
coverage generation. A coverage change invalidates the prior destination decision.

The selector allows at most 24 candidates, depth four, 16 transitions and one route request with
4096 expansions per decision. These limits also apply to prerequisites, alternatives and continuers.
Strategies use typed triggers and integer priority adjustments. Each Candidate carries a typed action
payload, reason and objective identity. Movement names Home or an Entity; Cast carries its spell and
target; Attack carries its target. There is no string registry.

The migration appends `controller = Legacy` and `scheduler_lag_micros = 0` to the roster and adds
runner, recovery scan and scheduler tables. Existing goals and action observations keep their schema and meaning.
The roster selector travels with the Character; runner observations and foreground work do not.
The Character delete operation removes the runner and recovery scan rows. Production publication still requires the
schema review described in LyraCore's `docs/danger-zones.md`.

The durable fixture now includes populated migration. Before running the complete ignored target,
build Module Wasm from core `be3fa67d0f0c24749230560544a3e8e8b577f61d` with collection
`5724de1660a2628e88320914bdd5abb0c69da517`, retain an immutable copy, and set
`PLAYERBOTS_PRECEDING_WASM` to that path. Package CI performs this build automatically. The case
publishes that preceding Wasm, populates real bot, goal, quest and action rows, then upgrades the
same private Standalone to the current Wasm. It records both Wasm identities and asserts the old
rows survive, selector defaults apply, and backfill takes bounded passes.
