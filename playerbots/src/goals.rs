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
//!
//! QUESTING. An ungrouped bot works quests around its home point: take one, kill what it names,
//! take what the kill leaves, hand it back. ONE rule decides whether a quest is worth walking to,
//! and it is the same rule the core applies when the bot arrives — [`quest_gate`] mirrors
//! `apply_accept_quest`'s own Refusals, in its order, and `crate::actor::accept_quest` is the
//! authority that answers for real. A bot that walked to a giver and was refused would walk there
//! again next second and forever; picking with the accept gate is what makes that impossible.

use spacetimedb::{ReducerContext, Table};

use super::{
    cond, goal, pkg_playerbots_bot, pkg_playerbots_goal, pkg_playerbots_personality,
    pkg_playerbots_rotation, PlayerbotsBot, PlayerbotsGoal, PlayerbotsPersonality,
    PlayerbotsRotation, ROLE_HEALER, ROLE_TANK,
};
use crate::{
    game_areatrigger_teleport, game_character, game_character_quest, game_corpse_loot,
    game_creature_quest, game_creature_spline, game_group, game_group_member, game_instance,
    game_melee_attack, game_quest_objective, game_quest_template, game_threat, game_world_entity,
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

/// How far a bot looks for a quest giver, an objective to kill, or something to grind. Two
/// 50-yard grid cells out, so one look costs a fixed handful of indexed cell reads and everything
/// the quest loop needs comes out of that one list.
const QUEST_SIGHT_YD: f32 = 60.0;

/// How far a bot ranges from its home point before it walks back. The leash is what keeps a
/// population a neighbourhood rather than a diaspora: a chase can drag a bot a long way, and a bot
/// past the leash walks home before it does anything else.
const QUEST_LEASH_YD: f32 = 150.0;

/// Close enough to be standing at the home point.
const HOME_ARRIVAL_YD: f32 = 10.0;

/// How close a bot stands before it talks to a quest giver. Well inside the core's own 10-yard
/// giver gate, so arriving is never the reason an accept or a turn-in is refused.
const INTERACT_RANGE_YD: f32 = 5.0;

/// How close a corpse has to be for a bot to empty it. Inside the core's 10-yard loot gate, and
/// inside melee reach — a bot that just killed something is already standing on it.
const LOOT_RANGE_YD: f32 = 5.0;

/// How many quests a bot works at once, far under the core's twenty-slot log.
///
/// A bot never abandons a quest, because abandoning deletes the log row and the log row is the
/// only thing that stops the bot picking the same quest again — dropping it is how the
/// re-selection loop gets back in. So a quest the bot cannot finish (an objective nothing in this
/// Package works, a target that does not live near its home point) holds its slot for good, and
/// this cap is what keeps one such quest from ending the bot's career instead of costing it a
/// third of its attention.
const BOT_QUEST_LOG_LIMIT: usize = 3;

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
        get_back_up(ctx, &me, bot, now);
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

    let engaged = combat_target(ctx, &me, party.as_ref());

    // A bot in a party takes its lead from the party; questing is what an ungrouped bot does with
    // itself. Keeping the branch here — after the crossing, before the plain fight — is what lets
    // a quester label its own fights, so an Operator reading the goal table sees a bot hunting a
    // quest target rather than an unattributed FIGHT.
    if party.is_none() {
        if let Some(kind) = quest(ctx, &me, bot, &personality, engaged) {
            record_goal(ctx, bot.character_guid, kind, now);
            return;
        }
    }

    if let Some(target) = engaged {
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

// ---- getting back up ---------------------------------------------------------------------------

/// The two steps between dying and standing up again, and the order they happen in. A dead bot has
/// no client to press the button, so the tick presses it: release to the graveyard, then resurrect
/// there. Pure, so "a dead bot always gets back up" is a property of a function rather than of a
/// live death.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DeathStep {
    /// Not dead. Nothing to do.
    None,
    /// Dead and still lying where it fell: release, which leaves a corpse and puts the ghost at
    /// the graveyard the death resolves to.
    Release,
    /// A ghost at the graveyard: resurrect there.
    Resurrect,
}

pub(crate) fn death_step(dead: bool, ghost: bool) -> DeathStep {
    match (dead, ghost) {
        (false, _) => DeathStep::None,
        (true, false) => DeathStep::Release,
        (true, true) => DeathStep::Resurrect,
    }
}

/// Get back up, one step per tick.
///
/// The quest log survives both steps — nothing in a death touches `game_character_quest` — so a
/// bot that died mid-quest resumes the quest it was on rather than choosing again. Where it
/// resumes from is the graveyard the death resolved to, which is why a quest area within the leash
/// of one is a quest area a bot can die in and carry on.
fn get_back_up(ctx: &ReducerContext, me: &crate::WorldEntity, bot: &PlayerbotsBot, now: i64) {
    use lyracore_shared::constants::player_flags;
    let step = death_step(me.dead, me.player_flags & player_flags::GHOST != 0);
    let outcome = match step {
        DeathStep::None => return,
        DeathStep::Release => crate::actor::repop(ctx, me.guid),
        DeathStep::Resurrect => crate::actor::spirit_res(ctx, me.guid),
    };
    record_goal(ctx, bot.character_guid, goal::RESURRECTING, now);
    match outcome {
        Ok(()) if step == DeathStep::Resurrect => spacetimedb::log::info!(
            "playerbots: bot {} resurrected at the graveyard and resumes what it was doing",
            me.guid
        ),
        Ok(()) => {}
        Err(refusal) => {
            spacetimedb::log::warn!(
                "playerbots: bot {} could not get back up: {refusal}",
                me.guid
            )
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

/// Does this bot close on what it fights and swing at it, or hold its ground and cast?
///
/// A healer holds its ground: it heals the party from where it stands, and a healer that ran into
/// the pack to swing at it would be a healer nobody could keep alive. With no party there is
/// nobody to hold it for — and a Priest that will not swing kills nothing, so it would finish no
/// quest and hold one goal forever. Ungrouped, every role fights. Pure.
pub(crate) fn closes_to_melee(role: u8, in_a_party: bool) -> bool {
    role != ROLE_HEALER || !in_a_party
}

fn fight(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    bot: &PlayerbotsBot,
    party: Option<&Party>,
    personality: &PlayerbotsPersonality,
    target: u64,
) {
    if closes_to_melee(bot.role, party.is_some()) {
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

// ---- questing ----------------------------------------------------------------------------------

/// The bot, as the accept gate sees it.
pub(crate) struct Questor {
    pub level: u32,
    pub race: u8,
    pub class: u8,
}

/// The `QuestTemplate` columns the accept gate reads, and no others. Built by [`Requirements::of`]
/// so the column names live in one place: if the core gate ever grows a sixth column, this struct
/// is where it lands and [`quest_gate`] is where it is answered.
pub(crate) struct Requirements {
    pub min_level: u32,
    pub races: u32,
    pub classes: u32,
    pub prev_quest: u32,
    pub repeatable: bool,
}

impl Requirements {
    fn of(tmpl: &crate::QuestTemplate) -> Self {
        Self {
            min_level: tmpl.min_level,
            races: tmpl.required_races,
            classes: tmpl.required_classes,
            prev_quest: tmpl.prev_quest_id,
            repeatable: tmpl.repeatable,
        }
    }
}

/// What the bot's quest log already says about a quest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogEntry {
    /// No row: the bot has never taken it.
    Absent,
    /// A row that is neither rewarded nor failed: the bot is on it now.
    Active,
    /// Turned in. Re-takable only when the quest is repeatable.
    Rewarded,
    /// Ran out of time. Re-takable whatever the quest says, exactly like the core's own reset.
    Failed,
}

/// Why a bot may not take a quest.
///
/// Every variant below `Open` mirrors one Refusal of `crate::quest::apply_accept_quest`, in that
/// reducer's own order, so the reason the bot names is the reason the core would have given. The
/// gates a bot cannot answer in advance — is the giver in range, does the giver offer this quest —
/// are not here: those are answered by walking there and asking.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum QuestGate {
    /// The core would accept this. Worth walking to.
    Open,
    TooLow,
    WrongRace,
    WrongClass,
    PrerequisiteUnmet,
    AlreadyHeld,
}

/// The accept gate, asked BEFORE the bot commits to a run rather than after it arrives.
///
/// This is the one decision the quester slice exists for. Selecting a quest without it re-chose an
/// un-acceptable chained quest every second: the bot ran to the giver, was refused for a
/// prerequisite it had never done, and ran there again. Selection and acceptance therefore ask the
/// same question, and `apply_accept_quest` stays the authority that answers it for real — this
/// only decides whether the walk is worth taking.
///
/// Pure, so every arm is a unit test rather than a live run.
pub(crate) fn quest_gate(
    who: &Questor,
    needs: &Requirements,
    prerequisite_rewarded: bool,
    logged: LogEntry,
) -> QuestGate {
    if who.level < needs.min_level {
        return QuestGate::TooLow;
    }
    if !lyracore_shared::quest::race_allowed(needs.races, who.race) {
        return QuestGate::WrongRace;
    }
    if !lyracore_shared::quest::class_allowed(needs.classes, who.class) {
        return QuestGate::WrongClass;
    }
    if needs.prev_quest != 0 && !prerequisite_rewarded {
        return QuestGate::PrerequisiteUnmet;
    }
    // The core resets a row in place rather than refusing it in exactly two cases: a repeatable
    // quest that was turned in, and a quest of any kind that ran out of time. Anything else that
    // already has a row is a duplicate.
    match logged {
        LogEntry::Absent | LogEntry::Failed => QuestGate::Open,
        LogEntry::Rewarded if needs.repeatable => QuestGate::Open,
        _ => QuestGate::AlreadyHeld,
    }
}

/// [`quest_gate`] over live rows. The prerequisite is `crate::quest::quest_is_rewarded` — the same
/// predicate `apply_accept_quest` calls for the same question — and the race and class masks go
/// through the same `lyracore_shared::quest` functions the core gate uses.
fn gate_for(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    tmpl: &crate::QuestTemplate,
    log: &[crate::CharacterQuest],
) -> QuestGate {
    quest_gate(
        &Questor {
            level: me.level,
            race: me.race(),
            class: me.class(),
        },
        &Requirements::of(tmpl),
        tmpl.prev_quest_id == 0
            || crate::quest::quest_is_rewarded(ctx, me.guid, tmpl.prev_quest_id),
        logged(log, tmpl.entry),
    )
}

fn logged(log: &[crate::CharacterQuest], quest_entry: u32) -> LogEntry {
    match log.iter().find(|row| row.quest_entry == quest_entry) {
        None => LogEntry::Absent,
        Some(row) if row.failed => LogEntry::Failed,
        Some(row) if row.rewarded => LogEntry::Rewarded,
        Some(_) => LogEntry::Active,
    }
}

/// Every row in this bot's quest log, oldest first. Read once per tick: the gate, the work and the
/// room-to-take-another decision all ask the same rows.
fn quest_log(ctx: &ReducerContext, character_guid: u64) -> Vec<crate::CharacterQuest> {
    let mut rows: Vec<crate::CharacterQuest> = ctx
        .db
        .game_character_quest()
        .by_character()
        .filter(&character_guid)
        .collect();
    rows.sort_by_key(|row| row.id);
    rows
}

/// What a bot does about quests this tick, or `None` when it found nothing to do and the caller
/// should carry on down its own list. The returned goal is what the tick records.
fn quest(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    bot: &PlayerbotsBot,
    personality: &PlayerbotsPersonality,
    engaged: Option<u64>,
) -> Option<u8> {
    let home = (bot.home_x, bot.home_y, bot.home_z);
    if distance_2d(me.x, me.y, home.0, home.1) > QUEST_LEASH_YD {
        let _ = crate::actor::stop_attack(ctx, me.guid);
        walk_toward(ctx, me, home, HOME_ARRIVAL_YD, true);
        return Some(goal::QUEST_TRAVEL);
    }
    let sight =
        crate::helpers::entities_near(ctx, me.map_id, me.instance_id, me.x, me.y, QUEST_SIGHT_YD);
    // Before anything else: a corpse decays, and a COLLECT objective is satisfied out of the bag.
    take_what_the_kill_left(ctx, me, &sight);

    let log = quest_log(ctx, me.guid);
    let active: Vec<&crate::CharacterQuest> = log
        .iter()
        .filter(|row| !row.rewarded && !row.failed)
        .collect();

    // Already swinging at something. Whether that is quest work is the bot's to say; anything else
    // is self-defence or a party assist, which the plain fight branch answers.
    if let Some(target) = engaged {
        return match engaged_reason(ctx, &active, target, held_kind(ctx, me.guid)) {
            Some(kind) => {
                fight(ctx, me, bot, None, personality, target);
                Some(kind)
            }
            None => None,
        };
    }

    for cq in &active {
        if let Some(kind) = work_quest(ctx, me, bot, personality, cq, &sight) {
            return Some(kind);
        }
    }
    if active.len() < BOT_QUEST_LOG_LIMIT {
        if let Some(kind) = take_a_quest(ctx, me, &log, &sight) {
            return Some(kind);
        }
    }
    grind(ctx, me, bot, personality, &sight)
}

/// The goal a fight already in progress belongs to, or `None` when it is not the quest loop's
/// fight at all. A creature one of the bot's quests names is quest work; a creature the bot picked
/// itself while grinding stays grinding, which the goal row is what remembers.
fn engaged_reason(
    ctx: &ReducerContext,
    active: &[&crate::CharacterQuest],
    target: u64,
    held: Option<u8>,
) -> Option<u8> {
    let entry = crate::helpers::live_entity(ctx, target).ok()?.entry;
    if active
        .iter()
        .any(|cq| kill_target_entry(ctx, cq) == Some(entry))
    {
        return Some(goal::QUEST_HUNT);
    }
    (held == Some(goal::GRIND)).then_some(goal::GRIND)
}

fn held_kind(ctx: &ReducerContext, character_guid: u64) -> Option<u8> {
    goal_of(ctx, character_guid).map(|row| row.kind)
}

/// The creature entry this bot still has to kill for `cq`, or `None` when every counted objective
/// has reached its required count.
///
/// Deliberately NOT a completeness test. `apply_turn_in_quest` owns completeness and answers a
/// COLLECT objective out of the live bag; this only decides whether there is still hunting to do,
/// which is what stops a bot culling wolves forever after the second one.
fn kill_target_entry(ctx: &ReducerContext, cq: &crate::CharacterQuest) -> Option<u32> {
    ctx.db
        .game_quest_objective()
        .by_quest()
        .filter(&cq.quest_entry)
        .find(|obj| {
            obj.kind == crate::quest::objective_kind::KILL_CREATURE
                && cq.counts.get(obj.obj_index as usize).copied().unwrap_or(0) < obj.required_count
        })
        .map(|obj| obj.target_entry)
}

/// One held quest's next step: hunt what it names, or carry it back to whoever ends it. `None`
/// means this quest has nothing the bot can do right now, and the caller tries the next one.
fn work_quest(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    bot: &PlayerbotsBot,
    personality: &PlayerbotsPersonality,
    cq: &crate::CharacterQuest,
    sight: &[crate::WorldEntity],
) -> Option<u8> {
    if let Some(entry) = kill_target_entry(ctx, cq) {
        let target = nearest(me, sight, |e| !e.is_player() && !e.dead && e.entry == entry)?;
        fight(ctx, me, bot, None, personality, target.guid);
        return Some(goal::QUEST_HUNT);
    }
    hand_it_back(ctx, me, bot, cq.quest_entry, sight)
}

/// Carry a worked quest back. The ender in sight is walked to and asked; no ender in sight means
/// the bot has strayed from the hub it took the quest at, and walking home is what brings the hub
/// back into view.
fn hand_it_back(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    bot: &PlayerbotsBot,
    quest_entry: u32,
    sight: &[crate::WorldEntity],
) -> Option<u8> {
    let ender = nearest(me, sight, |e| {
        !e.is_player()
            && !e.dead
            && offers(ctx, e.entry, quest_entry, crate::quest::quest_role::END)
    });
    let Some(ender) = ender else {
        let home = (bot.home_x, bot.home_y, bot.home_z);
        if distance_2d(me.x, me.y, home.0, home.1) <= HOME_ARRIVAL_YD {
            return None;
        }
        walk_toward(ctx, me, home, HOME_ARRIVAL_YD, true);
        return Some(goal::QUEST_TRAVEL);
    };
    if distance_2d(me.x, me.y, ender.x, ender.y) > INTERACT_RANGE_YD {
        walk_toward(
            ctx,
            me,
            (ender.x, ender.y, ender.z),
            INTERACT_RANGE_YD,
            true,
        );
        return Some(goal::QUEST_TRAVEL);
    }
    // Reward index 0: this Package takes the quest's guaranteed rewards and the first of any
    // choice, because a bot has no gear plan to pick against.
    match crate::actor::turn_in_quest(ctx, me.guid, ender.guid, quest_entry, 0) {
        Ok(()) => {
            spacetimedb::log::info!("playerbots: bot {} turned in quest {quest_entry}", me.guid);
            Some(goal::QUEST_TRAVEL)
        }
        // Not complete after all — a COLLECT objective the bot has not filled, most often. Nothing
        // more to do for this quest; the caller moves on to the next one.
        Err(_) => None,
    }
}

/// Take a quest from the nearest giver in sight that has one for this bot, walking to it first.
///
/// [`gate_for`] is what picks the quest, so a quest the core would refuse is never walked to. When
/// the core refuses one anyway the two have drifted apart, which is a defect rather than a
/// gameplay outcome, and the warning below is the only place it can show up.
///
/// NEAREST, not first-found. The bot re-picks every second while it walks, and the sight list is
/// ordered by grid cell rather than by distance — so picking the first match would hand the bot a
/// different giver each time its own movement shifted the cells, and it would shuttle between two
/// of them instead of reaching either. The nearest giver stays the nearest as the bot closes on it.
fn take_a_quest(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    log: &[crate::CharacterQuest],
    sight: &[crate::WorldEntity],
) -> Option<u8> {
    let giver = nearest(me, sight, |e| {
        !e.is_player() && !e.dead && !open_quests_of(ctx, me, e.entry, log).is_empty()
    })?;
    if distance_2d(me.x, me.y, giver.x, giver.y) > INTERACT_RANGE_YD {
        walk_toward(
            ctx,
            me,
            (giver.x, giver.y, giver.z),
            INTERACT_RANGE_YD,
            true,
        );
        return Some(goal::QUEST_TRAVEL);
    }
    for quest_entry in open_quests_of(ctx, me, giver.entry, log) {
        match crate::actor::accept_quest(ctx, me.guid, giver.guid, quest_entry) {
            Ok(()) => {
                spacetimedb::log::info!(
                    "playerbots: bot {} accepted quest {quest_entry} from creature {}",
                    me.guid,
                    giver.entry
                );
                return Some(goal::QUEST_TRAVEL);
            }
            Err(refusal) => spacetimedb::log::warn!(
                "playerbots: bot {} was refused quest {quest_entry}, which its own accept gate had \
                 opened: {refusal} — selection and acceptance have drifted apart",
                me.guid
            ),
        }
    }
    None
}

/// The quests creature template `entry` starts that this bot could take right now.
fn open_quests_of(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    entry: u32,
    log: &[crate::CharacterQuest],
) -> Vec<u32> {
    offered_by(ctx, entry, crate::quest::quest_role::START)
        .into_iter()
        .filter(|quest_entry| {
            ctx.db
                .game_quest_template()
                .entry()
                .find(quest_entry)
                .is_some_and(|tmpl| gate_for(ctx, me, &tmpl, log) == QuestGate::Open)
        })
        .collect()
}

/// Kill something for the experience. What a bot does when no quest it can take is on offer and
/// nothing it holds can be worked — a bot standing still in a field reads as broken.
///
/// `crate::faction::is_friendly` is the same predicate the attack core's own gate uses, so a bot
/// picks exactly the targets a player could: hostile and neutral, never green.
fn grind(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    bot: &PlayerbotsBot,
    personality: &PlayerbotsPersonality,
    sight: &[crate::WorldEntity],
) -> Option<u8> {
    let victim = nearest(me, sight, |e| {
        !e.is_player()
            && !e.dead
            && e.owner_guid == 0
            && !crate::faction::is_friendly(ctx, me.faction_template, e.faction_template)
            && offered_by(ctx, e.entry, crate::quest::quest_role::START).is_empty()
    })?;
    fight(ctx, me, bot, None, personality, victim.guid);
    Some(goal::GRIND)
}

/// Empty every corpse within reach: the coin first, then every slot the core will hand over. Costs
/// no tick — a bot that just killed something is already standing on it — and self-limiting,
/// because a looted corpse has no coin and no slots left to ask about.
fn take_what_the_kill_left(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    sight: &[crate::WorldEntity],
) {
    for corpse in sight
        .iter()
        .filter(|e| e.dead && !e.is_player() && distance_2d(me.x, me.y, e.x, e.y) <= LOOT_RANGE_YD)
    {
        if corpse.money > 0 {
            let _ = crate::actor::loot_money(ctx, me.guid, corpse.guid);
        }
        let slots: Vec<u8> = ctx
            .db
            .game_corpse_loot()
            .by_corpse()
            .filter(&corpse.guid)
            .map(|row| row.slot)
            .collect();
        for slot in slots {
            let _ = crate::actor::take_loot(ctx, me.guid, corpse.guid, slot);
        }
    }
}

/// The quests creature template `entry` offers in `role`. An indexed read off the relation table
/// itself, which is what makes "does this creature have anything for me" cost one lookup rather
/// than a walk over every quest on the realm.
fn offered_by(ctx: &ReducerContext, entry: u32, role: u8) -> Vec<u32> {
    ctx.db
        .game_creature_quest()
        .by_creature()
        .filter(&entry)
        .filter(|row| row.role == role)
        .map(|row| row.quest_entry)
        .collect()
}

fn offers(ctx: &ReducerContext, entry: u32, quest_entry: u32, role: u8) -> bool {
    offered_by(ctx, entry, role).contains(&quest_entry)
}

/// The closest entity in `sight` that `wanted` accepts.
fn nearest<'a>(
    me: &crate::WorldEntity,
    sight: &'a [crate::WorldEntity],
    mut wanted: impl FnMut(&crate::WorldEntity) -> bool,
) -> Option<&'a crate::WorldEntity> {
    sight
        .iter()
        .filter(|e| e.guid != me.guid && wanted(e))
        .fold(None, |best: Option<(&crate::WorldEntity, f32)>, e| {
            let d = distance_2d(me.x, me.y, e.x, e.y);
            match best {
                Some((_, bd)) if bd <= d => best,
                _ => Some((e, d)),
            }
        })
        .map(|(e, _)| e)
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

    // ---- getting back up -----------------------------------------------------------------------

    #[test]
    fn a_living_bot_has_no_death_to_recover_from() {
        assert_eq!(death_step(false, false), DeathStep::None);
        assert_eq!(death_step(false, true), DeathStep::None);
    }

    /// The whole recovery, in the order it happens: a fresh corpse releases, and the ghost that
    /// release produced resurrects. Two ticks, and the bot is standing.
    #[test]
    fn a_dead_bot_releases_and_then_resurrects() {
        assert_eq!(death_step(true, false), DeathStep::Release);
        assert_eq!(death_step(true, true), DeathStep::Resurrect);
    }

    // ---- who swings ----------------------------------------------------------------------------

    #[test]
    fn a_healer_in_a_party_stays_back_and_everyone_else_closes() {
        assert!(!closes_to_melee(ROLE_HEALER, true));
        assert!(closes_to_melee(ROLE_TANK, true));
    }

    /// A solo healer that would not swing kills nothing, so it finishes no quest objective and
    /// holds one goal for the rest of its life.
    #[test]
    fn an_ungrouped_healer_fights_like_anyone_else() {
        assert!(closes_to_melee(ROLE_HEALER, false));
    }

    // ---- the accept gate -----------------------------------------------------------------------

    const HUMAN_WARRIOR: Questor = Questor {
        level: 5,
        race: 1,
        class: 1,
    };

    /// A quest with no requirements at all — the shape most of the world's quests have.
    fn open_to_anyone() -> Requirements {
        Requirements {
            min_level: 0,
            races: 0,
            classes: 0,
            prev_quest: 0,
            repeatable: false,
        }
    }

    /// THE regression. A chained quest whose previous step the bot has never turned in was chosen
    /// over and over on an imported node: selection did not ask about the chain, so the bot ran to
    /// the giver, was refused, and ran there again the next second, forever. Selection asks now.
    #[test]
    fn a_chained_quest_is_never_chosen_before_its_previous_step_is_turned_in() {
        let chained = Requirements {
            prev_quest: 26,
            ..open_to_anyone()
        };
        assert_eq!(
            quest_gate(&HUMAN_WARRIOR, &chained, false, LogEntry::Absent),
            QuestGate::PrerequisiteUnmet
        );
        assert_eq!(
            quest_gate(&HUMAN_WARRIOR, &chained, true, LogEntry::Absent),
            QuestGate::Open,
            "the chain opens the moment its previous step is rewarded"
        );
    }

    #[test]
    fn a_quest_over_the_bots_level_is_never_chosen() {
        let needs_ten = Requirements {
            min_level: 10,
            ..open_to_anyone()
        };
        assert_eq!(
            quest_gate(&HUMAN_WARRIOR, &needs_ten, true, LogEntry::Absent),
            QuestGate::TooLow
        );
        let at_level = Questor {
            level: 10,
            ..HUMAN_WARRIOR
        };
        assert_eq!(
            quest_gate(&at_level, &needs_ten, true, LogEntry::Absent),
            QuestGate::Open
        );
    }

    /// The Northshire human chain carries `required_races` 77 — Human, Dwarf, Night Elf, Gnome. An
    /// Orc bot standing at the same giver must not choose it.
    #[test]
    fn a_quest_closed_to_the_bots_race_is_never_chosen() {
        let alliance_only = Requirements {
            races: 77,
            ..open_to_anyone()
        };
        assert_eq!(
            quest_gate(&HUMAN_WARRIOR, &alliance_only, true, LogEntry::Absent),
            QuestGate::Open
        );
        let orc = Questor {
            race: 2,
            ..HUMAN_WARRIOR
        };
        assert_eq!(
            quest_gate(&orc, &alliance_only, true, LogEntry::Absent),
            QuestGate::WrongRace
        );
    }

    #[test]
    fn a_quest_closed_to_the_bots_class_is_never_chosen() {
        let warrior_only = Requirements {
            classes: 1,
            ..open_to_anyone()
        };
        assert_eq!(
            quest_gate(&HUMAN_WARRIOR, &warrior_only, true, LogEntry::Absent),
            QuestGate::Open
        );
        let mage = Questor {
            class: 8,
            ..HUMAN_WARRIOR
        };
        assert_eq!(
            quest_gate(&mage, &warrior_only, true, LogEntry::Absent),
            QuestGate::WrongClass
        );
    }

    /// Holding a quest is the ONLY memory a bot has that it already chose one. A bot that could
    /// choose a quest it is already on would walk to the giver forever, which is the same loop by
    /// another route.
    #[test]
    fn a_quest_the_bot_is_already_on_is_never_chosen_again() {
        assert_eq!(
            quest_gate(&HUMAN_WARRIOR, &open_to_anyone(), true, LogEntry::Active),
            QuestGate::AlreadyHeld
        );
    }

    /// The core resets a row in place for exactly two cases rather than refusing it, and a bot
    /// that read those as duplicates would stop questing at a repeatable giver.
    #[test]
    fn only_a_repeatable_turn_in_and_a_run_out_timer_are_re_takable() {
        let once = open_to_anyone();
        let again = Requirements {
            repeatable: true,
            ..open_to_anyone()
        };
        assert_eq!(
            quest_gate(&HUMAN_WARRIOR, &once, true, LogEntry::Rewarded),
            QuestGate::AlreadyHeld
        );
        assert_eq!(
            quest_gate(&HUMAN_WARRIOR, &again, true, LogEntry::Rewarded),
            QuestGate::Open
        );
        assert_eq!(
            quest_gate(&HUMAN_WARRIOR, &once, true, LogEntry::Failed),
            QuestGate::Open,
            "a quest that ran out of time is re-takable whatever it says about repeating"
        );
    }

    /// The gate names the FIRST reason the core would have named, not any reason: a bot that is
    /// too low AND the wrong race hears "too low", exactly as the reducer would say it.
    #[test]
    fn the_gate_refuses_in_the_reducers_own_order() {
        let shut = Requirements {
            min_level: 60,
            races: 2,
            classes: 2,
            prev_quest: 26,
            repeatable: false,
        };
        assert_eq!(
            quest_gate(&HUMAN_WARRIOR, &shut, false, LogEntry::Active),
            QuestGate::TooLow
        );
        let low_gone = Requirements {
            min_level: 0,
            ..shut
        };
        assert_eq!(
            quest_gate(&HUMAN_WARRIOR, &low_gone, false, LogEntry::Active),
            QuestGate::WrongRace
        );
        let race_gone = Requirements {
            races: 0,
            ..low_gone
        };
        assert_eq!(
            quest_gate(&HUMAN_WARRIOR, &race_gone, false, LogEntry::Active),
            QuestGate::WrongClass
        );
        let class_gone = Requirements {
            classes: 0,
            ..race_gone
        };
        assert_eq!(
            quest_gate(&HUMAN_WARRIOR, &class_gone, false, LogEntry::Active),
            QuestGate::PrerequisiteUnmet
        );
        let chain_gone = Requirements {
            prev_quest: 0,
            ..class_gone
        };
        assert_eq!(
            quest_gate(&HUMAN_WARRIOR, &chain_gone, false, LogEntry::Active),
            QuestGate::AlreadyHeld
        );
    }

    // ---- the gate mirror does not drift --------------------------------------------------------

    /// Every Refusal `crate::quest::apply_accept_quest` can produce, and what answers it on this
    /// side. Five are selection gates the bot asks in advance; four are answered by the bot's own
    /// shape and are named here so the accounting is complete rather than partial.
    const CORE_ACCEPT_REFUSALS: &[(&str, &str)] = &[
        (
            "player not in world",
            "not a selection gate: a bot with no body never reaches the quest loop",
        ),
        (
            "dead players cannot accept quests",
            "not a selection gate: `get_back_up` takes the tick while the bot is dead",
        ),
        (
            "that quest giver does not offer this quest",
            "not a selection gate: `offered_by` reads the same relation rows to find the giver",
        ),
        (
            "no such quest",
            "not a selection gate: the template IS what selection reads",
        ),
        ("requires level", "QuestGate::TooLow"),
        ("quest not available to your race", "QuestGate::WrongRace"),
        ("quest not available to your class", "QuestGate::WrongClass"),
        (
            "must complete the prerequisite quest first",
            "QuestGate::PrerequisiteUnmet",
        ),
        (
            "already on or completed that quest",
            "QuestGate::AlreadyHeld",
        ),
    ];

    /// The forms `apply_accept_quest` produces a Refusal through. Counting them is what catches a
    /// gate this Package has never heard of.
    const REFUSAL_FORMS: &[&str] = &["Err(", "ok_or_else(", "map_err("];

    fn core_accept_body() -> String {
        let src = crate::test_scan::read_scanned("module/src/quest.rs")
            .expect("module/src/quest.rs is core, never an optional drop-in");
        crate::test_scan::code_of(&src, "pub(crate) fn apply_accept_quest(")
    }

    /// The one thing this whole slice exists for, as a test rather than as a promise: a gate the
    /// core applies and this Package does not is a gate the bot walks into, over and over.
    ///
    /// `crate::actor::accept_quest` stays the authority — nothing here can make a bot take a quest
    /// the core refuses. What this catches is the other direction: a gate that grows in the core
    /// and leaves selection choosing quests acceptance will not honour, which is exactly the shape
    /// the July foundation shipped with.
    #[test]
    fn the_accept_gate_mirror_accounts_for_every_refusal_the_core_can_give() {
        let body = core_accept_body();
        for (refusal, answered_by) in CORE_ACCEPT_REFUSALS {
            assert!(
                body.contains(refusal),
                "`apply_accept_quest` no longer refuses with \"{refusal}\" (mirrored here by \
                 {answered_by}). Either the Refusal was reworded — move this row with it — or the \
                 gate is gone, in which case drop the row and the `QuestGate` variant together."
            );
        }
        let produced: usize = REFUSAL_FORMS.iter().map(|f| body.matches(f).count()).sum();
        assert_eq!(
            produced,
            CORE_ACCEPT_REFUSALS.len(),
            "`apply_accept_quest` produces {produced} Refusals but this Package accounts for {}. \
             A bot picks a quest with `quest_gate` and then asks the core for it: a gate only the \
             core knows about is one the bot walks to the giver for and is refused at, every \
             second, forever — the defect this slice was written to close. Mirror the new gate in \
             `quest_gate` and add its row to CORE_ACCEPT_REFUSALS, or add the row with a written \
             reason why selection cannot ask it in advance.",
            CORE_ACCEPT_REFUSALS.len()
        );
    }

    /// The bot names the first reason the core would have named. That only holds while the two ask
    /// in the same order, so the order is pinned on the core side too.
    #[test]
    fn the_core_asks_its_selection_gates_in_the_order_this_package_mirrors() {
        let body = core_accept_body();
        let order = [
            "min_level",
            "race_allowed",
            "class_allowed",
            "prev_quest_id",
            "already on or completed that quest",
        ];
        let mut previous = 0;
        for marker in order {
            let at = body.find(marker).unwrap_or_else(|| {
                panic!(
                    "`apply_accept_quest` no longer names `{marker}` — the mirror in `quest_gate` \
                        is asking about a gate the core has moved or dropped"
                )
            });
            assert!(
                at > previous,
                "`apply_accept_quest` now asks `{marker}` out of the order `quest_gate` mirrors, \
                 so a bot would name a different reason than the core does"
            );
            previous = at;
        }
    }

    /// Teardown leaves zero rows. A quester writes two kinds of durable row a wandering bot never
    /// did — its quest log, and the corpse a death leaves — and both have to go when the Operator
    /// despawns the population. Neither is this Package's table, so this pins that the core sweeps
    /// them rather than adding a sweep of our own.
    #[test]
    fn a_despawn_takes_the_quest_log_and_the_corpse_with_it() {
        assert!(
            crate::CHARACTER_OWNED_TABLES.contains(&"game_character_quest"),
            "a bot's quest log is not swept when its Character is deleted, so despawning the \
             population would leave quest rows behind"
        );
        let src = crate::test_scan::read_scanned("module/src/world.rs")
            .expect("module/src/world.rs is core, never an optional drop-in");
        let body = crate::test_scan::code_of(&src, "pub(crate) fn cascade_delete_character(");
        assert!(
            body.contains("game_corpse()"),
            "`cascade_delete_character` no longer deletes the corpse, so a bot despawned as a \
             ghost would leave one standing in a field forever"
        );
    }
}
