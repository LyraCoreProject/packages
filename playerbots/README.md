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

`pkg_playerbots_personality` holds the part of a fight the rotation leaves open. The axis today is
the flee threshold: two bots on one rotation at the same health break off at different points.

## The mind

One `game_tick_pass!`, with each bot throttled to a decision a second. There is no Package-owned
schedule row, so a republish cannot leave the bots pointing at a reducer the new wasm no longer has.

Each decision, in order: put a body back on if the bot has none; get back up if dead; break off if
hurt past the personality threshold; follow a leader who has crossed into another map or instance of
this Shard; cross a Shard boundary when the party is not on this one at all; quest, if the bot is
ungrouped; fight what is on the party; otherwise follow the leader, or wander near home.

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
grinding ground cleared its own evidence on every excursion. Joining a party stops the clock as
well: a bot in a party does no quest work, so nothing there could ever clear it.

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

A bot takes coin from every corpse and items only when a quest it is holding asks for that item. It
cannot sell and it cannot destroy, so anything else it picked up it would keep for the rest of its
life, filling the bag that taking a quest needs room in, for copper it can never realise. Leaving
the trash on the corpse is what keeps the bag usable, and it is why the bot never has to reserve a
slot against its own looting.

A quest that hands an item over on accept is not chosen when the bag is full, because the core
refuses it there. Handing back is split. A quest with collected items to give back is always walked
to, because the turn-in removes them before it grants the reward, exactly so a full bag can still
finish a collect quest — and that is the only thing that ever gives a bot a slot back, so refusing
to set off would shut it. A quest with nothing to give back has nothing to free, so on a full bag it
is not walked to at all: that trip could only end in a Refusal.

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

- Bots do not send invites. A player invites them.
- Movement is a straight line. Bots do not use navigation data.
- A leader who logs out is a leader who is not on this Shard, so the bots go home after the wait.
- A party the leader has LEFT is led by whichever bot inherited it, and a bot leader is on this
  Shard by definition — so those bots hold where they are. Disband the party, or bring the leader
  back, to send them home.
- A crossing is aimed at a portal into the destination map. A dungeon map with no imported portal
  row is a dungeon the bots cannot follow anybody into; they wait, then go home.
- A bot in a party does not quest. It follows the leader, which is what a party is for.
- A bot quests within 150 yards of its home point and looks 60 yards ahead. Move the home point to
  move the patch.
- The objectives a bot works are the ones it can kill, and the ones it fills by looting what it
  kills. A quest made only of the two it cannot work is never taken. A quest that mixes the two IS
  taken, and holds one of its three slots until the Operator does something about it; the stall
  column is where that shows up.
- A bot takes the first reward choice, because it has no gear plan to pick against.
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
