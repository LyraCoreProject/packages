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

Each decision, in order: put a body back on if the bot has none; break off if hurt past the
personality threshold; follow a leader who has crossed into another map or instance of this Shard;
cross a Shard boundary when the party is not on this one at all; fight what is on the party;
otherwise follow the leader, or wander near home when ungrouped.

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

Every action leaves through a core operation the player path also uses — the actor verbs for attack,
stop, cast and invite-accept, and the shared creature leg writer for movement. The Package decides
what to do; the core decides whether it is allowed.

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
