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

use spacetimedb::{ReducerContext, Table};

use super::{
    cond, pkg_playerbots_bot, pkg_playerbots_personality, pkg_playerbots_rotation, PlayerbotsBot,
    PlayerbotsPersonality, PlayerbotsRotation, ROLE_HEALER, ROLE_TANK,
};
use crate::{
    game_character, game_creature_spline, game_group, game_group_member, game_melee_attack,
    game_threat, game_world_entity,
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

/// The party a bot is in: who leads it and who else is in it.
struct Party {
    leader_guid: u64,
    members: Vec<u64>,
}

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
        think(ctx, &bot);
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

fn think(ctx: &ReducerContext, bot: &PlayerbotsBot) {
    let Ok(me) = crate::helpers::live_entity(ctx, bot.character_guid) else {
        return;
    };
    if me.dead {
        return;
    }
    let personality = personality_of(ctx, bot.character_guid);
    if should_flee(me.health, me.max_health, personality.flee_at_pct) {
        let _ = crate::actor::stop_attack(ctx, me.guid);
        walk_toward(ctx, &me, (bot.home_x, bot.home_y, bot.home_z), 0.0, true);
        return;
    }

    let party = party_of(ctx, bot.character_guid);
    if let Some(party) = &party {
        // A leader who has crossed into an instance is followed before anything else: a bot left
        // outside is a bot fighting nothing, next to nobody.
        if follow_across_partitions(ctx, &me, party.leader_guid) {
            return;
        }
    }

    if let Some(target) = combat_target(ctx, &me, party.as_ref()) {
        fight(ctx, &me, bot, party.as_ref(), &personality, target);
        return;
    }

    match &party {
        Some(party) => follow_leader(ctx, &me, party.leader_guid),
        None => wander(ctx, &me, bot),
    }
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
        leader_guid: group.leader_guid,
        members,
    })
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

/// A leader on another map or in another instance: rebuild the bot's position there directly. A
/// real player crosses with a client handshake it has to answer; a bot has no client, so the
/// server-side move IS the whole crossing.
///
/// Returns `true` when it acted, so the caller stops thinking about this tick.
///
/// SHARD BOUNDARY: this reads the leader out of THIS Shard's tables. A leader who has crossed to
/// another Shard has no row here at all, so this sees nothing and the bot stays put.
fn follow_across_partitions(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    leader_guid: u64,
) -> bool {
    let Ok(leader) = crate::helpers::live_entity(ctx, leader_guid) else {
        return false;
    };
    if crate::helpers::in_same_partition(&leader, me.map_id, me.instance_id) {
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
}
