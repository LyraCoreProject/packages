//! Deadmines (map 36) choreography — the encounter kernel's first consumer.
//!
//! Unlike the retired EventAI-routed dungeons, the vanilla data carries NO
//! `ACTION_T_SET_INSTANCE_DATA` rows for any Deadmines creature (dump-verified against classic-db
//! z2815: their ACID rows are combat spells and yells only, which import generically — VanCleef's
//! yells and his 50%-HP allies summon need nothing from this Package). The door, cannon, and
//! Mr. Smite behavior lived in a C++ instance script upstream, so this Package rebuilds it on the
//! kernel's notify hooks and primitives:
//!
//! - `on_creature_death` — Rhahk'Zor, Sneed, and Gilnid each open their door; Sneed's Shredder
//!   ejects Sneed on destruction.
//! - `on_go_used` — firing the Defias Cannon breaches the Iron Clad Door.
//! - `on_hp_threshold` + `equip_swap` + `move_to_point` — Mr. Smite's stair scene: at 66% he
//!   runs to his chest and comes back dual-wielding, at 33% he switches to his two-hand hammer.
//!   His yells are this Package's own content; the source data has no text rows for them.
//!
//! Omitted from the scene: Smite's stomp stun (no vanilla data row backs it; revisit if a
//! curated spell mapping lands).

use spacetimedb::ReducerContext;

use crate::encounter::{self, ENCOUNTER_DONE};
use crate::{game_gameobject, game_world_entity};

pub(crate) const MAP_ID: u32 = 36;

// Creature entries (classic-db z2815).
pub(crate) const RHAHKZOR: u32 = 644;
pub(crate) const SNEEDS_SHREDDER: u32 = 642;
pub(crate) const SNEED: u32 = 643;
pub(crate) const GILNID: u32 = 1763;
pub(crate) const MR_SMITE: u32 = 646;

// Gameobject template entries (classic-db z2815).
pub(crate) const FACTORY_DOOR: u32 = 13965;
pub(crate) const MAST_ROOM_DOOR: u32 = 16400;
pub(crate) const FOUNDRY_DOOR: u32 = 16399;
pub(crate) const IRON_CLAD_DOOR: u32 = 16397;
pub(crate) const DEFIAS_CANNON: u32 = 16398;
pub(crate) const SMITES_CHEST: u32 = 144111;

// Mr. Smite's swap weapons (classic-db z2815 item entries).
pub(crate) const SMITES_REAVER: u32 = 5196;
pub(crate) const SMITES_MIGHTY_HAMMER: u32 = 7230;

// Package encounter ids on map 36 (kernel `game_encounter_state` keys, below the reserved band).
pub(crate) const ENCOUNTER_RHAHKZOR: u32 = 0;
pub(crate) const ENCOUNTER_SNEED: u32 = 1;
pub(crate) const ENCOUNTER_GILNID: u32 = 2;
pub(crate) const ENCOUNTER_SMITE: u32 = 3;
pub(crate) const ENCOUNTER_CANNON: u32 = 4;

// Smite's two stands: first weapon change at 66%, second at 33%.
pub(crate) const SMITE_FIRST_STAND_PCT: u8 = 66;
pub(crate) const SMITE_SECOND_STAND_PCT: u8 = 33;

const SMITE_AGGRO_YELL: &str = "We're under attack! A vast, ye swabs! Repel the invaders!";
const SMITE_FIRST_STAND_YELL: &str =
    "You landlubbers are tougher than I thought! I'll have to improvise!";
const SMITE_SECOND_STAND_YELL: &str = "D'ah! Now you're making me angry!";

crate::game_hook!(on_creature_spawn, fn smite_arms_his_thresholds(ctx, payload) {
    if payload.entry != MR_SMITE {
        return;
    }
    // Idempotent registrations; a respawn re-arming them is a no-op.
    if let Err(error) = encounter::watch_hp_threshold(ctx, MR_SMITE, SMITE_FIRST_STAND_PCT) {
        spacetimedb::log::warn!("deadmines: smite 66% watch failed: {error}");
    }
    if let Err(error) = encounter::watch_hp_threshold(ctx, MR_SMITE, SMITE_SECOND_STAND_PCT) {
        spacetimedb::log::warn!("deadmines: smite 33% watch failed: {error}");
    }
});

crate::game_hook!(on_aggro, fn smite_calls_to_arms(ctx, payload) {
    let Some(creature) = ctx.db.game_world_entity().guid().find(payload.creature_guid) else {
        return;
    };
    if creature.entry != MR_SMITE || creature.instance_id == 0 {
        return;
    }
    let _ = crate::chat::apply_send_chat(
        ctx,
        creature,
        crate::chat::CHAT_YELL,
        0,
        SMITE_AGGRO_YELL.to_string(),
    );
});

crate::game_hook!(on_creature_death, fn deadmines_boss_died(ctx, payload) {
    if payload.instance_id == 0 {
        return;
    }
    let instance_id = payload.instance_id;
    match payload.entry {
        RHAHKZOR => {
            open_door_logged(ctx, FACTORY_DOOR, instance_id);
            mark_done(ctx, instance_id, ENCOUNTER_RHAHKZOR);
        }
        SNEEDS_SHREDDER => eject_sneed(ctx, payload.creature_guid, instance_id),
        SNEED => {
            open_door_logged(ctx, MAST_ROOM_DOOR, instance_id);
            mark_done(ctx, instance_id, ENCOUNTER_SNEED);
        }
        GILNID => {
            open_door_logged(ctx, FOUNDRY_DOOR, instance_id);
            mark_done(ctx, instance_id, ENCOUNTER_GILNID);
        }
        MR_SMITE => mark_done(ctx, instance_id, ENCOUNTER_SMITE),
        _ => {}
    }
});

crate::game_hook!(on_go_used, fn defias_cannon_fired(ctx, payload) {
    if payload.go_entry != DEFIAS_CANNON || payload.instance_id == 0 {
        return;
    }
    open_door_logged(ctx, IRON_CLAD_DOOR, payload.instance_id);
    mark_done(ctx, payload.instance_id, ENCOUNTER_CANNON);
});

crate::game_hook!(on_hp_threshold, fn smite_changes_weapons(ctx, payload) {
    if payload.entry != MR_SMITE || payload.instance_id == 0 {
        return;
    }
    let (yell, main_hand, off_hand) = match payload.pct {
        SMITE_FIRST_STAND_PCT => (SMITE_FIRST_STAND_YELL, SMITES_REAVER, SMITES_REAVER),
        SMITE_SECOND_STAND_PCT => (SMITE_SECOND_STAND_YELL, SMITES_MIGHTY_HAMMER, 0),
        _ => return,
    };
    if let Some(smite) = ctx.db.game_world_entity().guid().find(payload.creature_guid) {
        let _ = crate::chat::apply_send_chat(
            ctx,
            smite,
            crate::chat::CHAT_YELL,
            0,
            yell.to_string(),
        );
    }
    // The run to his chest — the spot he rearms at. The live GO row anchors the leg, so the scene
    // follows the spawn data instead of a hardcoded point.
    if let Some(chest) = ctx
        .db
        .game_gameobject()
        .by_map()
        .filter(&MAP_ID)
        .find(|go| go.instance_id == payload.instance_id && go.template_entry == SMITES_CHEST)
    {
        if let Err(error) =
            encounter::move_to_point(ctx, payload.creature_guid, chest.x, chest.y, chest.z, true)
        {
            spacetimedb::log::warn!("deadmines: smite chest run failed: {error}");
        }
    } else {
        spacetimedb::log::warn!(
            "deadmines: no Smite's Chest in instance {} — weapon swap without the run",
            payload.instance_id
        );
    }
    if let Err(error) = encounter::equip_swap(ctx, payload.creature_guid, main_hand, off_hand, 0) {
        spacetimedb::log::warn!("deadmines: smite weapon swap failed: {error}");
    }
});

/// Sneed bursts out of his destroyed shredder: one tracked wave spawn at the wreck.
fn eject_sneed(ctx: &ReducerContext, shredder_guid: u64, instance_id: u64) {
    let Some(wreck) = ctx.db.game_world_entity().guid().find(shredder_guid) else {
        spacetimedb::log::warn!("deadmines: shredder corpse {shredder_guid} missing — no Sneed");
        return;
    };
    let spawned = encounter::spawn_wave(
        ctx,
        instance_id,
        ENCOUNTER_SNEED,
        MAP_ID,
        &[SNEED],
        wreck.x,
        wreck.y,
        wreck.z,
        wreck.orientation,
    );
    if spawned.is_empty() {
        spacetimedb::log::warn!("deadmines: Sneed did not spawn from the shredder wreck");
    }
}

fn open_door_logged(ctx: &ReducerContext, go_entry: u32, instance_id: u64) {
    if let Err(error) = encounter::open_door(ctx, go_entry, instance_id) {
        spacetimedb::log::warn!("deadmines: door {go_entry} did not open: {error}");
    }
}

fn mark_done(ctx: &ReducerContext, instance_id: u64, encounter_id: u32) {
    if let Err(error) = encounter::set_encounter_state(ctx, instance_id, encounter_id, ENCOUNTER_DONE) {
        spacetimedb::log::warn!("deadmines: encounter {encounter_id} state write failed: {error}");
    }
}
