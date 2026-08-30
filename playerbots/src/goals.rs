//! The bot mind: one tick pass, two notify hooks, and the movement a session-less Character needs
//! because no client is sending it any.
//!
//! WHAT DRIVES A BOT. A bot's `game_world_entity` row carries the PLAYER type mask, so the creature
//! behaviour cycle skips it — nothing in the core moves a bot, aggroes for it, or picks its spells.
//! This file is the whole of that. It runs on the core scheduler's tick pass, so there is no
//! Package-owned schedule row for a republish to leave pointing at a reducer that no longer exists.
//!
//! WHAT IT DOES NOT DO. Every action goes out through a core operation the player path also uses:
//! the actor verbs for attack, stop, cast and invite-accept, and the shared creature leg writer for
//! movement. The Package decides WHAT to do; the core decides whether it is allowed.
//!
//! CROSSING A SHARD BOUNDARY. A bot follows its party wherever the party goes, and on a realm of
//! several Shards that means a Transfer. The tick decides it and the Gateway drives it: the tick
//! writes one Transfer Intent, marks the bot in transit, and stops. The bot arrives with no live
//! entity and no goal, and the first tick there rebuilds it and falls it back in. Both halves are
//! this file; there is no arrival reducer.

use spacetimedb::{ReducerContext, Table};

use super::{
    cond, goal, pkg_playerbots_bot, pkg_playerbots_goal, pkg_playerbots_personality,
    pkg_playerbots_rotation, PlayerbotsBot, PlayerbotsGoal, PlayerbotsPersonality,
    PlayerbotsRotation, ROLE_HEALER, ROLE_TANK,
};
use crate::{
    game_areatrigger_teleport, game_character, game_creature_spline, game_group, game_group_member,
    game_instance, game_melee_attack, game_threat, game_world_entity,
};

/// How long a bot waits between decisions. The tick pass fires every half second; a bot that
/// re-decided that often would re-throw its movement leg before the last one had played.
const THINK_INTERVAL_MICROS: i64 = 1_000_000;

/// A grouped bot converges to within this distance of its leader.
pub(crate) const FOLLOW_RANGE_YD: f32 = 15.0;

/// How close a follow leg aims. Short of [`FOLLOW_RANGE_YD`], so a bot that arrives is comfortably
/// inside the range rather than oscillating on its edge.
const FOLLOW_STAND_OFF_YD: f32 = 8.0;

/// How far an ungrouped bot strays from its home point.
const WANDER_RADIUS_YD: f32 = 20.0;

/// A bot joins a fight that starts within this distance of it.
const ASSIST_RADIUS_YD: f32 = 30.0;

/// Melee reach. A bot closes to this before its swings can land.
const MELEE_RANGE_YD: f32 = 4.0;

/// How long a bot stands somewhere that is not its home ground, with no party to follow on this
/// Shard, before it crosses home.
///
/// It is a wait rather than an immediate decision because arriving on a Shard and being abandoned
/// on one look identical for a moment: a bot lands in a dungeon a heartbeat before the leader whose
/// crossing was driven first. A live crossing settles in milliseconds, so ten seconds separates the
/// two with room to spare.
const STRANDED_WAIT_MICROS: i64 = 10_000_000;

/// How long a bot stays bodiless after its Transfer Intent is written before the tick gives up on
/// the crossing and rebuilds it where it stands.
///
/// The Intent is a request, not a record. If nothing drives it — the Gateway is down, or a
/// republish landed in the middle — no Refusal comes back and no retry happens, so the only way out
/// is this deadline. On a realm of one Shard nothing ever drives it either, because the placement
/// WAS the whole crossing, and this is what puts the bot back in the world there.
const IN_TRANSIT_WAIT_MICROS: i64 = 3_000_000;

/// The party a bot is in: who leads it and who else is in it.
struct Party {
    group_id: u64,
    leader_guid: u64,
    members: Vec<u64>,
}

/// The pair a Shard Map routes on: which map, and which instance of it (`0` is the open world).
type Partition = (u32, u64);

crate::game_tick_pass!(fn playerbots_brain_pass(ctx) {
    // The Package's own ensure path. A Shard that has just published, or a second Shard that has
    // never run an Operator verb, seeds itself here rather than waiting to be told.
    super::ensure_defaults(ctx);
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    let bots = ctx.db.pkg_playerbots_bot();
    let due: Vec<PlayerbotsBot> = bots
        .iter()
        .filter(|bot| bot.next_think_micros <= now)
        .collect();
    for mut bot in due {
        think(ctx, &bot, now);
        bot.next_think_micros = now + THINK_INTERVAL_MICROS;
        bots.id().update(bot);
    }
});

// A group invite landed on a bot. A bot has no client to answer with, so the answer is server-side,
// through the same accept core a real client's accept reaches.
//
// PLANE NOTE: this hook fires where the invite row is written. On a realm whose party authority is a
// separate Shard the bot roster there is empty, this handler sees nothing, and the answer has to
// come from the Gateway instead. That is the Gateway's business, not the Package's.
crate::game_hook!(on_group_invite, fn playerbots_auto_accept(ctx, payload) {
    if !is_bot(ctx, payload.target_guid) {
        return;
    }
    // Named, not numbered. A bot has no client and no chat, so this line is the only record that an
    // invite was answered at all, and a wall of 15-digit guids is not a record anyone can read.
    // Deliberately NOT the in-transit-fenced lookup: a diagnostic wants the inviter's name even
    // while they are mid-Transfer, and nothing on this path writes to their Character.
    let inviter = ctx
        .db
        .game_character()
        .guid()
        .find(payload.inviter_guid)
        .map(|character| character.name)
        .unwrap_or_else(|| payload.inviter_guid.to_string());
    match crate::actor::accept_group_invite(ctx, payload.target_guid) {
        Ok(()) => spacetimedb::log::info!(
            "playerbots: bot {} accepted a party invite from {inviter}",
            payload.target_guid
        ),
        Err(refusal) => spacetimedb::log::warn!(
            "playerbots: bot {} could not accept the invite from {inviter}: {refusal}",
            payload.target_guid
        ),
    }
});

// A bot was hit. Hit back: a bot that stands still while something chews on it reads as broken long
// before anyone notices it has no brain for that case.
crate::game_hook!(on_damage_taken, fn playerbots_defend(ctx, payload) {
    if payload.attacker_guid == 0 || !is_bot(ctx, payload.target_guid) {
        return;
    }
    if ctx
        .db
        .game_melee_attack()
        .attacker_guid()
        .find(payload.target_guid)
        .is_some()
    {
        return; // already swinging at something; do not drop a fight to answer a stray hit
    }
    if let Err(refusal) = crate::actor::attack(ctx, payload.target_guid, payload.attacker_guid) {
        spacetimedb::log::warn!(
            "playerbots: bot {} could not defend against {}: {refusal}",
            payload.target_guid,
            payload.attacker_guid
        );
    }
});

fn is_bot(ctx: &ReducerContext, guid: u64) -> bool {
    ctx.db
        .pkg_playerbots_bot()
        .by_character()
        .filter(&guid)
        .next()
        .is_some()
}

// ---- the decision ----------------------------------------------------------------------------

fn think(ctx: &ReducerContext, bot: &PlayerbotsBot, now: i64) {
    let Some(me) = body(ctx, bot, now) else {
        return;
    };
    if me.dead {
        return;
    }
    let personality = personality_of(ctx, bot.character_guid);
    if should_flee(me.health, me.max_health, personality.flee_at_pct) {
        let _ = crate::actor::stop_attack(ctx, me.guid);
        walk_toward(ctx, &me, (bot.home_x, bot.home_y, bot.home_z), 0.0, true);
        record_goal(ctx, bot.character_guid, goal::FLEE, now);
        return;
    }

    let party = party_of(ctx, bot.character_guid);
    let leader = party
        .as_ref()
        .and_then(|party| crate::helpers::live_entity(ctx, party.leader_guid).ok());
    match &leader {
        // A leader who has crossed into an instance on THIS Shard is followed before anything
        // else: a bot left outside is a bot fighting nothing, next to nobody.
        Some(leader) => {
            if follow_across_partitions(ctx, &me, leader) {
                record_goal(ctx, bot.character_guid, goal::FOLLOW, now);
                return;
            }
        }
        // Nobody to follow on this Shard: the party crossed a boundary, this bot was left behind
        // by one, or its party is gone. All three are answered by a crossing of its own.
        None => {
            if cross_shards(ctx, &me, bot, party.as_ref(), now) {
                return;
            }
        }
    }

    if let Some(target) = combat_target(ctx, &me, party.as_ref()) {
        fight(ctx, &me, bot, party.as_ref(), &personality, target);
        record_goal(ctx, bot.character_guid, goal::FIGHT, now);
        return;
    }

    match &party {
        Some(party) => {
            follow_leader(ctx, &me, party.leader_guid);
            record_goal(ctx, bot.character_guid, goal::FOLLOW, now);
        }
        None => {
            wander(ctx, &me, bot);
            record_goal(ctx, bot.character_guid, goal::WANDER, now);
        }
    }
}

// ---- the goal row ------------------------------------------------------------------------------

fn goal_of(ctx: &ReducerContext, character_guid: u64) -> Option<PlayerbotsGoal> {
    ctx.db
        .pkg_playerbots_goal()
        .by_character()
        .filter(&character_guid)
        .next()
}

/// Write down what this bot is doing. Re-deciding the SAME goal leaves `since_micros` alone, so it
/// keeps measuring how long the bot has held the goal — which is what both crossing waits read.
fn record_goal(ctx: &ReducerContext, character_guid: u64, kind: u8, now: i64) {
    let goals = ctx.db.pkg_playerbots_goal();
    match goal_of(ctx, character_guid) {
        Some(row) if row.kind == kind => {}
        Some(mut row) => {
            row.kind = kind;
            row.since_micros = now;
            goals.id().update(row);
        }
        None => {
            goals.insert(PlayerbotsGoal {
                id: 0,
                character_guid,
                kind,
                since_micros: now,
            });
        }
    }
}

/// How long this bot has held `kind`, or `0` when it is holding something else.
fn held_for(ctx: &ReducerContext, character_guid: u64, kind: u8, now: i64) -> i64 {
    goal_of(ctx, character_guid)
        .filter(|row| row.kind == kind)
        .map_or(0, |row| now.saturating_sub(row.since_micros))
}

// ---- arrival adoption --------------------------------------------------------------------------

/// May the tick put a body back on a bodiless bot whose durable Character row is on this Shard?
///
/// Not while a Transfer Intent this Shard wrote is still young enough to be driven. The Gateway
/// reads the Character row to decide where the crossing goes, so a body put back before then would
/// move the bot out from under its own Intent.
///
/// Yes once the wait is over, whatever became of the crossing. An Intent is a request, not a
/// record: nothing refuses it and nothing retries it. So the wait is the only way back for a bot
/// whose crossing was never driven — a republish in the middle of one, a Gateway that was down —
/// and it is also the ordinary path on a realm of one Shard, where the placement WAS the whole
/// crossing and there was never anything for the Gateway to do.
///
/// Pure, because "no stuck bot" is the property this decides and a property is worth an assertion.
pub(crate) fn may_rebuild(goal_kind: Option<u8>, in_transit_for_micros: i64) -> bool {
    goal_kind != Some(goal::IN_TRANSIT) || in_transit_for_micros >= IN_TRANSIT_WAIT_MICROS
}

/// This bot's live entity, rebuilt from its durable Character row when it has none.
///
/// That rebuild IS the arrival. A Transfer carries the Character row and every registered transfer
/// arm across the Shard boundary; it does not carry a `game_world_entity`, and nothing on the far
/// side re-spawns one. Until this runs the arriving bot is durable but not in the world.
///
/// Two states deliberately produce no body:
///
/// - The bot is escrowed mid-Transfer. [`crate::helpers::character_by_guid`] is the in-transit
///   fence, so the durable row simply does not answer, and a bot half-way across a boundary is not
///   rebuilt on the Shard it is leaving.
/// - This Shard wrote a Transfer Intent for it inside [`IN_TRANSIT_WAIT_MICROS`]. The Gateway reads
///   the Character row to decide where the crossing goes, so putting a body back before the crossing
///   is driven would move the bot out from under its own Intent.
fn body(ctx: &ReducerContext, bot: &PlayerbotsBot, now: i64) -> Option<crate::WorldEntity> {
    if let Ok(me) = crate::helpers::live_entity(ctx, bot.character_guid) {
        return Some(me);
    }
    let current = goal_of(ctx, bot.character_guid);
    if !may_rebuild(
        current.as_ref().map(|row| row.kind),
        held_for(ctx, bot.character_guid, goal::IN_TRANSIT, now),
    ) {
        return None;
    }
    let character = crate::helpers::character_by_guid(ctx, bot.character_guid)?;
    let entity = crate::build_player_entity(ctx, &character, spacetimedb::Identity::ZERO);
    ctx.db.game_world_entity().insert(entity);
    // A leg thrown before the crossing would interpolate the arrival straight back across the map
    // it just left.
    ctx.db
        .game_creature_spline()
        .guid()
        .delete(bot.character_guid);
    spacetimedb::log::info!(
        "playerbots: bot {} is in the world on map {} instance {}",
        bot.character_guid,
        character.map_id,
        character.pending_instance_id
    );
    crate::helpers::live_entity(ctx, bot.character_guid).ok()
}

/// Break off at or below `flee_at_pct` of maximum health. `0` never flees, which is what a tank
/// wants; `100` always does, which is what a coward wants. Pure, so the divergence between two
/// bots on one rotation is a property of a function rather than of a live fight.
pub(crate) fn should_flee(health: u32, max_health: u32, flee_at_pct: u8) -> bool {
    if flee_at_pct == 0 || max_health == 0 {
        return false;
    }
    u64::from(health) * 100 <= u64::from(max_health) * u64::from(flee_at_pct)
}

fn personality_of(ctx: &ReducerContext, guid: u64) -> PlayerbotsPersonality {
    ctx.db
        .pkg_playerbots_personality()
        .by_character()
        .filter(&guid)
        .next()
        .unwrap_or(PlayerbotsPersonality {
            id: 0,
            character_guid: guid,
            flee_at_pct: 0,
            heal_at_pct: 0,
        })
}

fn party_of(ctx: &ReducerContext, guid: u64) -> Option<Party> {
    let membership = ctx
        .db
        .game_group_member()
        .by_character()
        .filter(&guid)
        .next()?;
    let group = ctx.db.game_group().group_id().find(membership.group_id)?;
    let members = ctx
        .db
        .game_group_member()
        .by_group()
        .filter(&membership.group_id)
        .map(|row| row.character_guid)
        .collect();
    Some(Party {
        group_id: group.group_id,
        leader_guid: group.leader_guid,
        members,
    })
}

// ---- crossing a Shard boundary -------------------------------------------------------------------

/// What a bot does when there is nobody to follow on this Shard.
///
/// Pure over facts the tick reads off its own Shard's rows, because that is all a Package ever
/// gets: the Module is Shard-agnostic, so "the party is somewhere else" is never a directory
/// lookup, it is the absence of the leader plus whatever durable trace the party left behind.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Crossing {
    /// Nothing to cross. The bot goes on thinking about this Shard.
    Stay,
    /// Into the party's live instance. The leader walked through a portal and this Shard kept the
    /// instance row that says which dungeon they went to.
    Join(Partition),
    /// Back to the bot's home point. Nothing is left here to follow.
    GoHome,
    /// Off its home ground with nobody to follow, and not for long enough to give up on them.
    Wait,
}

/// `party_instance` is the party's live instance AS THIS SHARD RECORDS IT. The Shard a party sets
/// out from keeps the row that resolved their portal, so a bot left behind can read where they
/// went; the Shard that serves the dungeon holds a mirror of the same instance under no party at
/// all, so a bot already inside reads `None` and falls through to the way home. That asymmetry is
/// what makes one rule serve both directions.
pub(crate) fn plan_crossing(
    here: Partition,
    party_instance: Option<Partition>,
    home: Partition,
    stranded_for_micros: i64,
) -> Crossing {
    if let Some(destination) = party_instance {
        if destination != here {
            return Crossing::Join(destination);
        }
    }
    if here == home {
        return Crossing::Stay;
    }
    if stranded_for_micros >= STRANDED_WAIT_MICROS {
        Crossing::GoHome
    } else {
        Crossing::Wait
    }
}

/// Act on [`plan_crossing`]. Returns `true` when it took the tick.
///
/// Waiting takes the tick too. A bot holding still for a few seconds while it finds out whether it
/// has been left behind is honest about not knowing, and it keeps the wait on one clock: the goal
/// row measures how long the bot has held ONE goal, so a bot that went back to fighting between
/// ticks would restart its wait every time.
fn cross_shards(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    bot: &PlayerbotsBot,
    party: Option<&Party>,
    now: i64,
) -> bool {
    let here = (me.map_id, me.instance_id);
    let home = (bot.home_map, 0);
    let destination = party_destination(ctx, party);
    let plan = plan_crossing(
        here,
        destination.as_ref().map(|d| (d.map_id, d.instance_id)),
        home,
        held_for(ctx, bot.character_guid, goal::STRANDED, now),
    );
    match plan {
        Crossing::Stay => false,
        Crossing::Wait => {
            record_goal(ctx, bot.character_guid, goal::STRANDED, now);
            true
        }
        Crossing::Join(_) => {
            let destination = destination.expect("Join is only planned from a known destination");
            cross(
                ctx,
                bot.character_guid,
                destination,
                "following the party",
                now,
            );
            true
        }
        Crossing::GoHome => {
            cross(
                ctx,
                bot.character_guid,
                crate::transfer::Destination {
                    map_id: bot.home_map,
                    instance_id: 0,
                    x: bot.home_x,
                    y: bot.home_y,
                    z: bot.home_z,
                    o: 0.0,
                },
                "the party is gone from this Shard",
                now,
            );
            true
        }
    }
}

/// Ask for one crossing. The Character row is what the Gateway reads to decide where the bot is
/// bound, and the core writer moves it and records the Intent in one transaction — so nothing here
/// may touch the bot's position afterwards, or the Intent reads as stale and the crossing is
/// refused.
fn cross(
    ctx: &ReducerContext,
    bot_guid: u64,
    destination: crate::transfer::Destination,
    reason: &str,
    now: i64,
) {
    let _ = crate::actor::stop_attack(ctx, bot_guid);
    // A leg still playing would fight the placement for as long as the client interpolates it.
    ctx.db.game_creature_spline().guid().delete(bot_guid);
    spacetimedb::log::info!(
        "playerbots: bot {bot_guid} crosses to map {} instance {} ({reason})",
        destination.map_id,
        destination.instance_id
    );
    crate::transfer::emit_bot_transfer_intent(ctx, bot_guid, destination, reason);
    record_goal(ctx, bot_guid, goal::IN_TRANSIT, now);
}

/// Where this bot's party went, as a place a Transfer can be aimed at.
///
/// Both halves have to be known. The instance row says WHICH dungeon; the portal that targets that
/// map says where inside it a party lands, which is the one point on a dungeon map the imported
/// game data guarantees a follower can stand. An instance nothing can land in is not a destination,
/// and a bot facing one is treated as having no party here at all — it waits, then goes home.
fn party_destination(
    ctx: &ReducerContext,
    party: Option<&Party>,
) -> Option<crate::transfer::Destination> {
    let party = party?;
    let instance = ctx
        .db
        .game_instance()
        .by_party()
        .filter(&party.group_id)
        .find(|instance| !instance.reset_requested)?;
    let (x, y, z, o) = portal_into(ctx, instance.map_id)?;
    Some(crate::transfer::Destination {
        map_id: instance.map_id,
        instance_id: instance.instance_id,
        x,
        y,
        z,
        o,
    })
}

/// Where the portal into `map_id` puts whoever walks through it. `game_areatrigger_teleport` records
/// each portal by its TARGET, so the rows that name this map are exactly the ways in, and they all
/// land in the same doorway.
fn portal_into(ctx: &ReducerContext, map_id: u32) -> Option<(f32, f32, f32, f32)> {
    ctx.db
        .game_areatrigger_teleport()
        .iter()
        .find(|portal| portal.target_map == map_id)
        .map(|portal| (portal.x, portal.y, portal.z, portal.o))
}

// ---- fighting --------------------------------------------------------------------------------

/// What this bot should be swinging at: whatever it already fights, or whatever has opened on it
/// or on somebody in its party nearby.
fn combat_target(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    party: Option<&Party>,
) -> Option<u64> {
    let melee = ctx.db.game_melee_attack();
    if let Some(row) = melee.attacker_guid().find(me.guid) {
        return Some(row.target_guid);
    }
    let party_guids: &[u64] = party.map(|p| p.members.as_slice()).unwrap_or(&[]);
    for candidate in
        crate::helpers::entities_near(ctx, me.map_id, me.instance_id, me.x, me.y, ASSIST_RADIUS_YD)
    {
        if candidate.is_player() || candidate.dead || candidate.owner_guid != 0 {
            continue;
        }
        let Some(row) = melee.attacker_guid().find(candidate.guid) else {
            continue;
        };
        if row.target_guid == me.guid || party_guids.contains(&row.target_guid) {
            return Some(candidate.guid);
        }
    }
    None
}

fn fight(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    bot: &PlayerbotsBot,
    party: Option<&Party>,
    personality: &PlayerbotsPersonality,
    target: u64,
) {
    // A healer holds its ground: it heals the party from where it stands, and a healer that ran
    // into the pack to swing at it would be a healer nobody could keep alive.
    if bot.role != ROLE_HEALER {
        let _ = crate::actor::attack(ctx, me.guid, target);
        // Nothing chases for a PLAYER-typed entity, so a bot that pulled from range would stand
        // there swinging at nothing. Close first, cast second.
        if let Ok(enemy) = crate::helpers::live_entity(ctx, target) {
            if distance_2d(me.x, me.y, enemy.x, enemy.y) > MELEE_RANGE_YD {
                walk_toward(ctx, me, (enemy.x, enemy.y, enemy.z), MELEE_RANGE_YD, true);
            }
        }
    }
    cast_rotation(ctx, me, bot, party, personality, target);
}

/// Walk the `(class, role)` rotation in priority order and cast the first row whose condition holds
/// and whose cast the core accepts. Cooldowns, the global cooldown, range, cost and the level gate
/// are all the cast core's answers — a Refusal here just means the next row gets its turn.
fn cast_rotation(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    bot: &PlayerbotsBot,
    party: Option<&Party>,
    personality: &PlayerbotsPersonality,
    target: u64,
) {
    let mut rows: Vec<PlayerbotsRotation> = ctx
        .db
        .pkg_playerbots_rotation()
        .by_class_role()
        .filter((bot.class, bot.role))
        .collect();
    rows.sort_by_key(|row| (row.priority, row.id));
    for row in rows {
        let Some(cast_at) = rotation_target(ctx, me, party, personality, &row, target) else {
            continue;
        };
        if crate::actor::cast_at(ctx, me.guid, row.spell_id, cast_at).is_ok() {
            return;
        }
    }
}

/// Whom a rotation row should be cast at, or `None` when its condition does not hold. The
/// condition and the target are one answer: a heal that fires without knowing who is hurt would
/// have to guess, and a peel that fires without knowing who is being hit would peel nothing.
fn rotation_target(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    party: Option<&Party>,
    personality: &PlayerbotsPersonality,
    row: &PlayerbotsRotation,
    current_target: u64,
) -> Option<u64> {
    match row.condition {
        cond::ALWAYS => Some(current_target),
        cond::SELF_MISSING_AURA => {
            (!crate::spell::has_aura(ctx, me.guid, row.spell_id)).then_some(me.guid)
        }
        cond::ENEMY_ON_ALLY => enemy_on_ally(ctx, me, party),
        cond::ALLY_HP_BELOW_PCT => {
            // The row's own threshold is the rotation's business; the bot's personality is the
            // healer's. Take the lower of the two, so a timid healer never out-heals its row and a
            // generous row never overrides a healer that was told to hold back.
            let threshold = row.threshold_pct.min(if personality.heal_at_pct == 0 {
                row.threshold_pct
            } else {
                personality.heal_at_pct
            });
            lowest_hurt_ally(ctx, me, party, threshold)
        }
        cond::ALLY_MISSING_AURA => ally_missing_aura(ctx, me, party, row.spell_id),
        cond::TANK_ENGAGED => tank_target(ctx, party),
        cond::ENEMIES_ENGAGED_GE_N => (engaged_enemies(ctx, me, party)
            >= usize::from(row.threshold_pct))
        .then_some(current_target),
        _ => None,
    }
}

/// Every party member's live entity, or just this bot's when it is alone. The bot itself is always
/// in the list: a healer is a party member too.
fn party_entities(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    party: Option<&Party>,
) -> Vec<crate::WorldEntity> {
    let Some(party) = party else {
        return crate::helpers::live_entity(ctx, me.guid)
            .into_iter()
            .collect();
    };
    party
        .members
        .iter()
        .filter_map(|guid| crate::helpers::live_entity(ctx, *guid).ok())
        .filter(|entity| {
            !entity.dead && crate::helpers::in_same_partition(entity, me.map_id, me.instance_id)
        })
        .collect()
}

/// A hostile unit swinging at somebody in the party who is not this bot — the peel target.
fn enemy_on_ally(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    party: Option<&Party>,
) -> Option<u64> {
    let allies: Vec<u64> = party_entities(ctx, me, party)
        .iter()
        .map(|entity| entity.guid)
        .filter(|guid| *guid != me.guid)
        .collect();
    if allies.is_empty() {
        return None;
    }
    let melee = ctx.db.game_melee_attack();
    for ally in allies {
        if let Some(row) = melee.by_target().filter(&ally).next() {
            return Some(row.attacker_guid);
        }
    }
    None
}

/// The most hurt party member at or below `threshold_pct`. Triage: the lowest first, so a healer
/// with one cast spends it where it counts.
fn lowest_hurt_ally(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    party: Option<&Party>,
    threshold_pct: u8,
) -> Option<u64> {
    party_entities(ctx, me, party)
        .into_iter()
        .filter(|entity| {
            entity.max_health > 0
                && u64::from(entity.health) * 100
                    <= u64::from(entity.max_health) * u64::from(threshold_pct)
        })
        .min_by_key(|entity| {
            (u64::from(entity.health) * 1000 / u64::from(entity.max_health.max(1))) as u32
        })
        .map(|entity| entity.guid)
}

fn ally_missing_aura(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    party: Option<&Party>,
    spell_id: u32,
) -> Option<u64> {
    party_entities(ctx, me, party)
        .into_iter()
        .find(|entity| !crate::spell::has_aura(ctx, entity.guid, spell_id))
        .map(|entity| entity.guid)
}

/// What the party's tank is fighting. The damage rotation's assist rule: hit what the tank holds,
/// so the party's damage lands on one target and the tank keeps it.
fn tank_target(ctx: &ReducerContext, party: Option<&Party>) -> Option<u64> {
    let party = party?;
    let melee = ctx.db.game_melee_attack();
    for guid in &party.members {
        let is_tank = ctx
            .db
            .pkg_playerbots_bot()
            .by_character()
            .filter(guid)
            .next()
            .is_some_and(|bot| bot.role == ROLE_TANK);
        if !is_tank {
            continue;
        }
        if let Some(row) = melee.attacker_guid().find(*guid) {
            return Some(row.target_guid);
        }
    }
    None
}

/// How many distinct hostile units the party currently holds threat on.
fn engaged_enemies(ctx: &ReducerContext, me: &crate::WorldEntity, party: Option<&Party>) -> usize {
    let threat = ctx.db.game_threat();
    let mut seen: Vec<u64> = Vec::new();
    for entity in party_entities(ctx, me, party) {
        for row in threat.by_source().filter(&entity.guid) {
            if !seen.contains(&row.creature_guid) {
                seen.push(row.creature_guid);
            }
        }
    }
    seen.len()
}

// ---- following and wandering -------------------------------------------------------------------

/// A leader on another map or in another instance OF THIS SHARD: rebuild the bot's position there
/// directly. A real player crosses with a client handshake it has to answer; a bot has no client,
/// so the server-side move IS the whole crossing.
///
/// Returns `true` when it acted, so the caller stops thinking about this tick.
///
/// SHARD BOUNDARY: the caller resolved `leader` out of THIS Shard's tables. A leader who has crossed
/// to another Shard has no row here at all, so this is never reached for one — [`cross_shards`]
/// answers that case instead.
fn follow_across_partitions(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    leader: &crate::WorldEntity,
) -> bool {
    if crate::helpers::in_same_partition(leader, me.map_id, me.instance_id) {
        return false;
    }
    let _ = crate::actor::stop_attack(ctx, me.guid);
    // A leg that was playing on the old map would interpolate the bot across the world.
    ctx.db.game_creature_spline().guid().delete(me.guid);
    let Ok(mut moved) = crate::helpers::live_entity(ctx, me.guid) else {
        return false;
    };
    moved.map_id = leader.map_id;
    moved.instance_id = leader.instance_id;
    place(&mut moved, leader.x, leader.y, leader.z);
    ctx.db.game_world_entity().guid().update(moved);
    if let Some(mut character) = crate::helpers::character_by_guid(ctx, me.guid) {
        character.map_id = leader.map_id;
        character.x = leader.x;
        character.y = leader.y;
        character.z = leader.z;
        ctx.db.game_character().guid().update(character);
    }
    true
}

fn follow_leader(ctx: &ReducerContext, me: &crate::WorldEntity, leader_guid: u64) {
    let Ok(leader) = crate::helpers::live_entity(ctx, leader_guid) else {
        return;
    };
    if distance_2d(me.x, me.y, leader.x, leader.y) <= FOLLOW_RANGE_YD {
        return;
    }
    walk_toward(
        ctx,
        me,
        (leader.x, leader.y, leader.z),
        FOLLOW_STAND_OFF_YD,
        true,
    );
}

/// An ungrouped bot mills about near its home point, so a realm with bots in it looks inhabited
/// rather than staged.
fn wander(ctx: &ReducerContext, me: &crate::WorldEntity, bot: &PlayerbotsBot) {
    let window = ctx.timestamp.to_micros_since_unix_epoch() / WANDER_LEG_MICROS;
    let (dx, dy) = wander_offset(bot.character_guid, window);
    let dest = (bot.home_x + dx, bot.home_y + dy, bot.home_z);
    walk_toward(ctx, me, dest, 0.0, false);
}

/// How long a bot holds one wander heading. A bot that re-rolled its destination every time it
/// thought would jitter on the spot instead of strolling anywhere.
const WANDER_LEG_MICROS: i64 = 8_000_000;

/// The offset from home this bot wanders to during `window`. Derived from the bot's own guid, so
/// two bots standing on the same home point do not walk in step; derived from the window, so one
/// bot holds a heading for a whole leg. Pure — no stored destination to keep in step with anything.
pub(crate) fn wander_offset(character_guid: u64, window: i64) -> (f32, f32) {
    // A cheap integer mix. It only has to spread headings; nothing here needs a real generator.
    let mixed = character_guid
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add((window as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9));
    let angle = (mixed % 360) as f32 * std::f32::consts::PI / 180.0;
    let reach = ((mixed >> 17) % 101) as f32 / 100.0 * WANDER_RADIUS_YD;
    (angle.cos() * reach, angle.sin() * reach)
}

// ---- movement --------------------------------------------------------------------------------

/// One movement leg toward `dest`, stopping `stand_off` yards short of it, capped at what the bot
/// can cover before it next thinks. Writes the same `game_creature_spline` row every creature leg
/// writes, so the Gateway relays it through the one movement path it already has.
fn walk_toward(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    dest: (f32, f32, f32),
    stand_off: f32,
    run: bool,
) {
    let speed = if run {
        lyracore_shared::constants::speeds::RUN
    } else {
        lyracore_shared::constants::speeds::WALK
    };
    let full = distance_2d(me.x, me.y, dest.0, dest.1);
    let step = step_length(full, stand_off, speed, THINK_INTERVAL_MICROS);
    if step <= 0.0 {
        return;
    }
    let (dx, dy) = ((dest.0 - me.x) / full, (dest.1 - me.y) / full);
    let landing = (
        me.x + dx * step,
        me.y + dy * step,
        // Interpolate height along the leg. Off-navigation ground is the accepted ceiling here:
        // a bot walks the straight line, exactly as a scripted creature leg does.
        me.z + (dest.2 - me.z) * (step / full),
    );
    let now_ms = (ctx.timestamp.to_micros_since_unix_epoch() / 1000) as u32;
    // A non-increasing spline id is dropped by the client, so two legs inside one transaction need
    // the second to out-number the first.
    let spline_id = ctx
        .db
        .game_creature_spline()
        .guid()
        .find(me.guid)
        .map_or(now_ms, |last| now_ms.max(last.spline_id.wrapping_add(1)));
    crate::creatures::tick::emit_move_spline(
        ctx,
        me.guid,
        (me.x, me.y, me.z),
        landing,
        ((step / speed) * 1000.0) as u32,
        run,
        spline_id,
        me.map_id,
        me.instance_id,
        (me.grid_x, me.grid_y),
    );
    let Ok(mut moved) = crate::helpers::live_entity(ctx, me.guid) else {
        return;
    };
    place(&mut moved, landing.0, landing.1, landing.2);
    moved.orientation = dy.atan2(dx);
    moved.last_move_ms = now_ms;
    ctx.db.game_world_entity().guid().update(moved);
}

/// How far this leg travels: the distance left after the stand-off, capped by what `speed` covers
/// in one think interval. Zero when the bot is already close enough, so an arrived bot emits no
/// leg at all. Pure — the arithmetic that decides whether a follow converges or oscillates.
pub(crate) fn step_length(distance: f32, stand_off: f32, speed: f32, interval_micros: i64) -> f32 {
    let remaining = distance - stand_off;
    if remaining <= 0.0 || speed <= 0.0 || !distance.is_finite() {
        return 0.0;
    }
    let reach = speed * (interval_micros as f32 / 1_000_000.0);
    remaining.min(reach)
}

/// Write a position and everything the spatial indexes derive from it. One place, so a move can
/// never leave the grid columns naming a cell the entity is no longer in.
fn place(entity: &mut crate::WorldEntity, x: f32, y: f32, z: f32) {
    let (grid_x, grid_y) = lyracore_shared::spatial::grid_cell(x, y);
    entity.x = x;
    entity.y = y;
    entity.z = z;
    entity.grid_x = grid_x;
    entity.grid_y = grid_y;
    entity.cell = lyracore_shared::spatial::grid_cell_id(grid_x, grid_y);
}

fn distance_2d(ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    (dx * dx + dy * dy).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flee_threshold_of_zero_never_breaks_off() {
        assert!(!should_flee(1, 100, 0));
        assert!(!should_flee(0, 100, 0));
    }

    #[test]
    fn a_bot_breaks_off_at_or_below_its_own_threshold() {
        assert!(should_flee(15, 100, 15));
        assert!(should_flee(10, 100, 15));
        assert!(!should_flee(16, 100, 15));
    }

    /// The party-brains divergence in one assertion: same health, same rotation, different bot.
    #[test]
    fn two_bots_at_the_same_health_diverge_on_the_flee_threshold_alone() {
        let (health, max_health) = (20, 100);
        assert!(!should_flee(health, max_health, 0));
        assert!(should_flee(health, max_health, 95));
    }

    #[test]
    fn an_arrived_follower_emits_no_leg() {
        assert_eq!(step_length(8.0, 8.0, 7.0, THINK_INTERVAL_MICROS), 0.0);
        assert_eq!(step_length(3.0, 8.0, 7.0, THINK_INTERVAL_MICROS), 0.0);
    }

    #[test]
    fn a_leg_never_overshoots_its_stand_off() {
        assert_eq!(step_length(10.0, 8.0, 7.0, THINK_INTERVAL_MICROS), 2.0);
    }

    #[test]
    fn a_long_chase_is_capped_at_one_intervals_travel() {
        assert_eq!(step_length(500.0, 8.0, 7.0, THINK_INTERVAL_MICROS), 7.0);
    }

    /// The follow acceptance is "within 15 yards after a ~78 yard hop, inside 30 seconds". The step
    /// arithmetic has to make that reachable before any live run can.
    #[test]
    fn a_seventy_eight_yard_hop_converges_well_inside_the_follow_range() {
        let mut distance = 78.0_f32;
        let mut ticks = 0;
        while distance > FOLLOW_RANGE_YD && ticks < 30 {
            distance -= step_length(
                distance,
                FOLLOW_STAND_OFF_YD,
                lyracore_shared::constants::speeds::RUN,
                THINK_INTERVAL_MICROS,
            );
            ticks += 1;
        }
        assert!(
            distance <= FOLLOW_RANGE_YD,
            "a follow that cannot close 78 yards in 30 thinks cannot pass its acceptance: \
             {distance} yards left after {ticks} thinks"
        );
    }

    #[test]
    fn a_wander_never_strays_past_its_radius() {
        for guid in [1_u64, 42, 9_999_999_999, u64::MAX] {
            for window in 0..64_i64 {
                let (dx, dy) = wander_offset(guid, window);
                let reach = (dx * dx + dy * dy).sqrt();
                assert!(
                    reach <= WANDER_RADIUS_YD + 0.001,
                    "guid {guid} window {window} wandered {reach} yards from home"
                );
            }
        }
    }

    #[test]
    fn a_bot_holds_one_heading_for_a_whole_leg_and_then_changes_it() {
        assert_eq!(wander_offset(7, 3), wander_offset(7, 3));
        assert_ne!(wander_offset(7, 3), wander_offset(7, 4));
    }

    #[test]
    fn two_bots_on_one_home_point_do_not_walk_in_step() {
        assert_ne!(wander_offset(7, 3), wander_offset(8, 3));
    }

    #[test]
    fn an_immobile_bot_travels_nothing() {
        assert_eq!(step_length(50.0, 0.0, 0.0, THINK_INTERVAL_MICROS), 0.0);
    }

    // ---- crossing a Shard boundary -----------------------------------------------------------

    /// The open world of the Shard a party sets out from, and the Deadmines instance they walk
    /// into. `HOME` is where a bot was spawned, which is where it goes when there is nothing left
    /// to follow.
    const HOME: Partition = (0, 0);
    const DUNGEON: Partition = (36, 7);

    #[test]
    fn a_bot_whose_party_walked_into_a_dungeon_crosses_after_them() {
        assert_eq!(
            plan_crossing(HOME, Some(DUNGEON), HOME, 0),
            Crossing::Join(DUNGEON)
        );
    }

    /// The warning the crossing seam carries: an Intent for a bot that is already on the
    /// destination Shard is refused, and costs a log line every tick it is written.
    #[test]
    fn a_bot_already_in_its_partys_instance_asks_for_no_crossing() {
        assert_eq!(
            plan_crossing(DUNGEON, Some(DUNGEON), HOME, 0),
            Crossing::Wait
        );
    }

    /// The return leg. The Shard that serves a dungeon holds the instance under no party, so a bot
    /// inside one reads no party instance at all — and once the leader is gone from it too, home is
    /// the only place left.
    #[test]
    fn a_bot_left_in_a_dungeon_goes_home_once_the_wait_is_over() {
        assert_eq!(
            plan_crossing(DUNGEON, None, HOME, STRANDED_WAIT_MICROS),
            Crossing::GoHome
        );
    }

    /// The arrival window: a bot is driven across on its own, so it can land a moment before the
    /// leader whose crossing was driven first. Turning round immediately would be a loop.
    #[test]
    fn a_bot_that_has_just_arrived_waits_for_its_party_rather_than_turning_round() {
        assert_eq!(
            plan_crossing(DUNGEON, None, HOME, STRANDED_WAIT_MICROS - 1),
            Crossing::Wait
        );
    }

    #[test]
    fn a_bot_standing_on_its_own_home_ground_never_crosses() {
        assert_eq!(plan_crossing(HOME, None, HOME, 0), Crossing::Stay);
        assert_eq!(
            plan_crossing(HOME, None, HOME, STRANDED_WAIT_MICROS * 100),
            Crossing::Stay
        );
    }

    /// Two parties in two instances of one map are two destinations, so the instance has to be part
    /// of the comparison — a bot in instance 7 whose party is in instance 8 has to cross.
    #[test]
    fn two_instances_of_one_map_are_two_destinations() {
        assert_eq!(
            plan_crossing(DUNGEON, Some((36, 8)), HOME, 0),
            Crossing::Join((36, 8))
        );
    }

    /// A crossing that was never driven — a republish in the middle of one, or a Gateway that was
    /// down — leaves a bot durable, bodiless and marked in transit, with no Intent row left (the
    /// core reaps those in a second). The wait is what puts it back in the world.
    #[test]
    fn a_bot_whose_crossing_was_never_driven_is_put_back_in_the_world() {
        assert!(!may_rebuild(Some(goal::IN_TRANSIT), 0));
        assert!(!may_rebuild(
            Some(goal::IN_TRANSIT),
            IN_TRANSIT_WAIT_MICROS - 1
        ));
        assert!(may_rebuild(Some(goal::IN_TRANSIT), IN_TRANSIT_WAIT_MICROS));
    }

    /// Arrival adoption: a Transfer does not carry the goal row, so an arriving bot holds no goal
    /// at all — and that is the state the tick has to rebuild a body for, immediately.
    #[test]
    fn a_bot_that_arrives_with_no_goal_is_rebuilt_at_once() {
        assert!(may_rebuild(None, 0));
    }

    /// A bodiless bot that is not crossing is a bot whose Shard despawned it — the same rebuild,
    /// with no wait to serve.
    #[test]
    fn a_bodiless_bot_holding_any_other_goal_is_rebuilt_at_once() {
        for kind in [
            goal::FOLLOW,
            goal::FIGHT,
            goal::FLEE,
            goal::WANDER,
            goal::STRANDED,
        ] {
            assert!(may_rebuild(Some(kind), 0), "goal kind {kind}");
        }
    }
}
