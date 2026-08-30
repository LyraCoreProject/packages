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

Each decision, in order: break off if hurt past the personality threshold; follow a leader who has
crossed into another map or instance; fight what is on the party; otherwise follow the leader, or
wander near home when ungrouped.

Every action leaves through a core operation the player path also uses — the actor verbs for attack,
stop, cast and invite-accept, and the shared creature leg writer for movement. The Package decides
what to do; the core decides whether it is allowed.

## Limits

- Bots do not send invites. A player invites them.
- A bot follows its leader across maps and instances **inside one Shard**. A leader who crosses to
  another Shard has no row where the bots are, so the bots stay put.
- Movement is a straight line. Bots do not use navigation data.
