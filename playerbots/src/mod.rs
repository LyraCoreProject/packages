//! The `playerbots` Package: a standing population of session-less Characters that a real player
//! can group with, so a small realm still has a party to test content with.
//!
//! A bot is a REAL Character on a Package-minted Account. It has a `game_character` row, a live
//! `game_world_entity` row with the PLAYER type mask, a spellbook, and durable position. What it
//! does not have is a Session: nothing ever calls `player_login` for it. That one difference is the
//! whole design. Because the bot is durable, it survives a Gateway restart and a republish. Because
//! it has no Session, the Gateway's session-less paths already treat it correctly: it can be invited
//! by name, and it refuses whispers.
//!
//! This file owns the roster: Accounts, names, the class/role tables, the Operator verbs, and the
//! despawn sweep. `goals.rs` owns the mind.
//!
//! WHY NO SCHEDULE ROW: the bot mind runs on one `game_tick_pass!`, which the core scheduler owns.
//! A Package that schedules itself writes a scheduled row, and a republish leaves that row pointing
//! at a reducer the new wasm no longer has — the bots then stand still with nothing in the log to
//! say why. With no Package-owned schedule row there is nothing a republish can break.

use spacetimedb::{reducer, table, Identity, ReducerContext, Table};

mod goals;
pub(crate) use goals::*;

use crate::package_config::game_package_config;
use crate::{game_account, game_character, game_world_entity};

/// The Package Config namespace and the Package name the Trust Review prints.
pub(crate) const PACKAGE: &str = "playerbots";

// ---- roles ---------------------------------------------------------------------------------

/// A bot's job in a party. The Operator passes these as the `role` argument, so the numbers are
/// part of the verb signature and must not be reordered.
pub(crate) const ROLE_TANK: u8 = 0;
pub(crate) const ROLE_HEALER: u8 = 1;
pub(crate) const ROLE_DPS: u8 = 2;

/// The class a role spawn picks when the Operator names no class. One per role, chosen so the three
/// together form a working vanilla party.
const DEFAULT_CLASS_FOR_ROLE: [(u8, u8); 3] = [
    (ROLE_TANK, class::WARRIOR),
    (ROLE_HEALER, class::PRIEST),
    (ROLE_DPS, class::MAGE),
];

/// Vanilla class ids. Only the classes this Package can kit are named.
pub(crate) mod class {
    pub(crate) const WARRIOR: u8 = 1;
    pub(crate) const PALADIN: u8 = 2;
    pub(crate) const PRIEST: u8 = 5;
    pub(crate) const MAGE: u8 = 8;
}

/// Every bot is a Human. Human can be Warrior, Paladin, Priest and Mage, so one race covers every
/// class this Package kits and the appearance fields stay a single constant.
const BOT_RACE: u8 = 1;

/// The most Characters one Account may hold. `create_character` enforces this itself; the roster
/// mints its next Account before it hits the refusal rather than after.
const CHARACTERS_PER_ACCOUNT: usize = 10;

// ---- rotation conditions -------------------------------------------------------------------

/// When a rotation row is allowed to fire. A row is data, so an Operator can change a bot's
/// behaviour with one SQL UPDATE and no republish; the condition is the part of that row the brain
/// reads before it decides.
pub(crate) mod cond {
    /// Fire whenever the spell is off cooldown and the bot has a target.
    pub(crate) const ALWAYS: u8 = 0;
    /// Fire when a hostile unit is attacking a party member other than this bot.
    pub(crate) const ENEMY_ON_ALLY: u8 = 1;
    /// Fire when this bot does not already carry the spell's own aura.
    pub(crate) const SELF_MISSING_AURA: u8 = 2;
    /// Fire when a party member is below `threshold_pct` health. Targets that member.
    pub(crate) const ALLY_HP_BELOW_PCT: u8 = 3;
    /// Fire when a party member does not carry the spell's aura. Targets that member.
    pub(crate) const ALLY_MISSING_AURA: u8 = 4;
    /// Fire when a party member with the tank role is engaged. Targets what the tank fights.
    pub(crate) const TANK_ENGAGED: u8 = 5;
    /// Fire when at least `threshold_pct` hostile units are engaged with the party.
    pub(crate) const ENEMIES_ENGAGED_GE_N: u8 = 6;
}

// ---- tables ---------------------------------------------------------------------------------

/// The bot roster: one row per live bot. Deleting this row is not enough to remove a bot — see
/// [`playerbots_despawn_all`], which deletes the Character the row points at. The row carries the
/// bot's home point so an ungrouped bot has somewhere to wander around, and its next think time so
/// the brain pass can skip most bots on most ticks. [entity]
#[table(
    accessor = pkg_playerbots_bot,
    public,
    index(accessor = by_character, btree(columns = [character_guid]))
)]
pub struct PlayerbotsBot {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    /// One row per bot. Not a database `#[unique]`: the roster needs a btree index it can `filter`
    /// on for the delete and transport sweeps, and a column cannot carry both. Uniqueness holds by
    /// construction — one insert per created Character, one sweep per deletion.
    pub character_guid: u64,
    pub account_id: u64,
    pub class: u8,
    pub role: u8,
    pub home_map: u32,
    pub home_x: f32,
    pub home_y: f32,
    pub home_z: f32,
    /// Wall-clock microseconds before which the brain pass leaves this bot alone.
    pub next_think_micros: i64,
}

// A despawned bot takes its roster row with it. The row is keyed by `character_guid`, so the
// indexed delete is the whole sweep. Delete-only: the row carries no owner identity to restamp.
crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_bot(ctx, character_guid) {
    let bots = ctx.db.pkg_playerbots_bot();
    for bot in bots.by_character().filter(&character_guid).collect::<Vec<_>>() {
        bots.id().delete(bot.id);
    }
});

// A bot that crosses a Shard boundary carries its roster row: the row IS what makes the Character a
// bot, so a bot that arrived without it would be an ordinary offline Character standing in a field
// forever. The `id` is a per-database surrogate, so the destination mints its own.
crate::character_owned!(transfer, fn sweep_transfer_pkg_playerbots_bot(ctx, character_guid, io) {
    table = pkg_playerbots_bot,
    by = by_character,
    remint = id,
});

// ---- goals -----------------------------------------------------------------------------------

/// What a bot is doing. The brain pass writes one of these every time its decision changes, so an
/// Operator can read a party's intent out of the table rather than out of the log.
pub(crate) mod goal {
    /// Keeping station on the party leader.
    pub(crate) const FOLLOW: u8 = 0;
    /// Swinging at whatever the party is fighting.
    pub(crate) const FIGHT: u8 = 1;
    /// Broken off past the personality threshold, running for the home point.
    pub(crate) const FLEE: u8 = 2;
    /// Ungrouped and near home, milling about.
    pub(crate) const WANDER: u8 = 3;
    /// Off its home ground with no party to follow on this Shard. `since_micros` opens the wait
    /// before the bot crosses home.
    pub(crate) const STRANDED: u8 = 4;
    /// A Transfer Intent is out for this bot. It has no live entity until the crossing settles.
    pub(crate) const IN_TRANSIT: u8 = 5;
    /// Running to a quest giver to take a quest, back to the one that ends it, or back inside the
    /// ground the bot quests on.
    pub(crate) const QUEST_TRAVEL: u8 = 6;
    /// Working a quest objective: killing what the quest names, and taking what it leaves.
    pub(crate) const QUEST_HUNT: u8 = 7;
    /// No quest work available, so killing for experience instead.
    pub(crate) const GRIND: u8 = 8;
    /// Dead: releasing to the graveyard and resurrecting there.
    pub(crate) const RESURRECTING: u8 = 9;
}

/// The bot's current goal, held between ticks. One row per bot that has decided anything at all.
///
/// The ABSENCE of a row is load-bearing: it is what a bot looks like the moment it arrives on a
/// Shard, because this row does not cross a Shard boundary (see the transport arm below). The
/// brain pass then rebuilds the bot's live entity and decides afresh, which is the whole of
/// arrival adoption — there is no arrival reducer. [entity]
#[table(
    accessor = pkg_playerbots_goal,
    public,
    index(accessor = by_character, btree(columns = [character_guid]))
)]
pub struct PlayerbotsGoal {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    /// One row per bot, by the same construction as the roster's own key.
    pub character_guid: u64,
    /// One of [`goal`].
    pub kind: u8,
    /// When the bot took this goal, in wall-clock microseconds. Held across ticks that re-decide
    /// the same goal, so it measures how long the bot has been doing this and not how long ago it
    /// last thought. The two waits the crossing needs — the arrival grace and the in-transit
    /// grace — are read off it.
    pub since_micros: i64,
    /// When the bot last held quests it could make no progress on, in wall-clock microseconds.
    /// `0` means it is getting on with things.
    ///
    /// A separate column because [`Self::since_micros`] cannot answer this. That one restarts every
    /// time the goal CHANGES, so a bot flapping between two kinds — walking back for a turn-in it
    /// will be refused, then grinding, then walking back — looks brand new on every tick it is
    /// read. This one is only cleared by real quest work, so it is the column an Operator sorts by
    /// to find a stuck bot. End-appended with a default, so a published Shard migrates in place.
    #[default(0i64)]
    pub stalled_since_micros: i64,
    /// Where this bot last accepted a quest, and whether it has accepted one at all.
    ///
    /// A bot ranges further than it can see, so the giver it took a quest from is often out of
    /// sight by the time the quest is done. Without somewhere to walk back to, that quest could
    /// never be handed in and would hold its slot for good. Not a place in the world so much as a
    /// bookmark: it is dropped by a Shard crossing with the rest of this row, and the bot simply
    /// takes its next quest somewhere else.
    #[default(false)]
    pub hub_known: bool,
    #[default(0.0f32)]
    pub hub_x: f32,
    #[default(0.0f32)]
    pub hub_y: f32,
    #[default(0.0f32)]
    pub hub_z: f32,
}

crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_goal(ctx, character_guid) {
    let goals = ctx.db.pkg_playerbots_goal();
    for row in goals.by_character().filter(&character_guid).collect::<Vec<_>>() {
        goals.id().delete(row.id);
    }
});

// A goal does NOT cross a Shard boundary, and that is the design rather than an omission. Every
// goal this Package can hold names a place: a leader to keep station on, a creature to swing at, a
// home point to run for. None of those exist on the destination Shard, so carrying the row would
// hand an arriving bot a decision about a world it is no longer in. Leaving it behind is also the
// arrival signal — a bot with no goal is a bot that has just landed, and the brain pass falls it
// back in with the party it finds there.
crate::character_owned!(not_transported, fn sweep_transfer_pkg_playerbots_goal());

/// How one bot fights when the rotation leaves a choice. The minimal axis is the flee threshold:
/// two bots on the same rotation at the same health diverge on it alone, which is what makes a
/// party of bots read as a party of people rather than one mind in three bodies. [entity]
#[table(
    accessor = pkg_playerbots_personality,
    public,
    index(accessor = by_character, btree(columns = [character_guid]))
)]
pub struct PlayerbotsPersonality {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    /// One row per bot, by the same construction as the roster's own key.
    pub character_guid: u64,
    /// Break off and run home at or below this share of maximum health. `0` never flees.
    pub flee_at_pct: u8,
    /// Heal a party member at or below this share of maximum health. `0` never heals.
    pub heal_at_pct: u8,
}

crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_personality(ctx, character_guid) {
    let rows = ctx.db.pkg_playerbots_personality();
    for row in rows.by_character().filter(&character_guid).collect::<Vec<_>>() {
        rows.id().delete(row.id);
    }
});

// A personality travels with its bot. Leaving it behind would hand the arriving bot the role
// defaults again and quietly undo whatever the Operator had tuned.
crate::character_owned!(transfer, fn sweep_transfer_pkg_playerbots_personality(ctx, character_guid, io) {
    table = pkg_playerbots_personality,
    by = by_character,
    remint = id,
});

/// What a `(class, role)` bot LEARNS at spawn. The kit is also the legality answer: a `(class,
/// role)` pair with no kit rows is a pairing this Package cannot fill, and the spawn verb refuses
/// it by name. [static]
#[table(
    accessor = pkg_playerbots_kit,
    public,
    index(accessor = by_class_role, btree(columns = [class, role]))
)]
pub struct PlayerbotsKit {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub class: u8,
    pub role: u8,
    pub spell_id: u32,
}

/// What a `(class, role)` bot CASTS, in priority order. Rows, not code: one class carries three
/// different fights because three sets of rows say so, and an Operator retunes a fight with a SQL
/// UPDATE while the realm is up. Every `spell_id` here must also be a kit row for the same pair,
/// or the bot would never have learned it. [static]
#[table(
    accessor = pkg_playerbots_rotation,
    public,
    index(accessor = by_class_role, btree(columns = [class, role]))
)]
pub struct PlayerbotsRotation {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub class: u8,
    pub role: u8,
    /// Lower fires first.
    pub priority: u8,
    pub spell_id: u32,
    /// One of [`cond`].
    pub condition: u8,
    /// The condition's number: a health share for the HP conditions, a count for
    /// [`cond::ENEMIES_ENGAGED_GE_N`], ignored otherwise.
    pub threshold_pct: u8,
}

// ---- Package Config ---------------------------------------------------------------------------

/// The Operator-tunable values, seeded once per Shard. Strings, because that is what the Config
/// surface stores; this Package parses its own.
const CONFIG_DEFAULTS: [(&str, &str); 6] = [
    // What `playerbots_populate` tops the population up to. Nothing populates on its own, so this
    // value only matters when the Operator asks.
    ("population_count", "10"),
    // Where a populated bot spawns and where an ungrouped bot wanders back to (Elwynn Forest).
    ("home_x", "-8930.0"),
    ("home_y", "-250.0"),
    ("home_z", "80.0"),
    // The level band a populated bot is created at.
    ("level_min", "1"),
    ("level_max", "10"),
];

fn config_value(ctx: &ReducerContext, key: &str) -> Option<String> {
    ctx.db
        .game_package_config()
        .by_package_key()
        .filter((PACKAGE, key))
        .next()
        .map(|row| row.value)
}

/// Read one Config value, falling back to the seeded default when the Operator has deleted the row
/// or written something this Package cannot parse. Operator configuration is untrusted input: a
/// typo must leave the bots working, not wedge the brain pass.
fn config_parsed<T: std::str::FromStr>(ctx: &ReducerContext, key: &str, fallback: T) -> T {
    config_value(ctx, key)
        .and_then(|raw| raw.trim().parse::<T>().ok())
        .unwrap_or(fallback)
}

/// The Package's own ensure path: seed the Config defaults and the class/role data if this Shard
/// has never seen them. Called from every Operator verb and from the brain pass, so a freshly
/// published Shard is ready without an install step and a second Shard seeds itself the first time
/// its tick runs.
///
/// Idempotent twice over. The Config seeding helper only inserts when the row is absent, so an
/// Operator's edited value survives a republish. The data seeding is skipped entirely once any kit
/// row exists, so the common case costs one count.
pub(crate) fn ensure_defaults(ctx: &ReducerContext) {
    for (key, value) in CONFIG_DEFAULTS {
        crate::package_config::ensure_package_config_default(ctx, PACKAGE, key, value);
    }
    if ctx.db.pkg_playerbots_kit().count() > 0 {
        return;
    }
    seed_class_role_data(ctx);
}

/// The shipped `(class, role)` rotations, and the kit each one implies.
///
/// `(class, role, priority, spell_id, condition, threshold_pct)`. The Paladin trio is the proof
/// that the table, not the code, decides the fight: one class, three roles, three rotations.
const DEFAULT_ROTATIONS: &[(u8, u8, u8, u32, u8, u8)] = &[
    // Warrior tank: taunt what is hitting somebody else, otherwise build threat.
    (class::WARRIOR, ROLE_TANK, 0, 355, cond::ENEMY_ON_ALLY, 0),
    (class::WARRIOR, ROLE_TANK, 1, 7386, cond::ALWAYS, 0),
    // Priest healer: a heal over time first, a direct heal when a member is really hurt.
    (
        class::PRIEST,
        ROLE_HEALER,
        0,
        2050,
        cond::ALLY_HP_BELOW_PCT,
        50,
    ),
    (
        class::PRIEST,
        ROLE_HEALER,
        1,
        139,
        cond::ALLY_HP_BELOW_PCT,
        80,
    ),
    // Mage damage.
    (class::MAGE, ROLE_DPS, 0, 133, cond::ALWAYS, 0),
    // Paladin tank: vanilla Paladins have no taunt, so the peel is a stun on whatever is hitting a
    // party member. Consecration needs a ground-target engine the core does not have yet, so the
    // row is seeded and simply never passes its condition today.
    (class::PALADIN, ROLE_TANK, 0, 853, cond::ENEMY_ON_ALLY, 0),
    (
        class::PALADIN,
        ROLE_TANK,
        1,
        20154,
        cond::SELF_MISSING_AURA,
        0,
    ),
    (
        class::PALADIN,
        ROLE_TANK,
        2,
        26573,
        cond::ENEMIES_ENGAGED_GE_N,
        3,
    ),
    (class::PALADIN, ROLE_TANK, 3, 20271, cond::ALWAYS, 0),
    // Paladin healer: heal the hurt, then keep the party blessed.
    (
        class::PALADIN,
        ROLE_HEALER,
        0,
        635,
        cond::ALLY_HP_BELOW_PCT,
        80,
    ),
    (
        class::PALADIN,
        ROLE_HEALER,
        1,
        19740,
        cond::ALLY_MISSING_AURA,
        0,
    ),
    // Paladin damage: keep the seal up, spend it on what the tank holds.
    (
        class::PALADIN,
        ROLE_DPS,
        0,
        20154,
        cond::SELF_MISSING_AURA,
        0,
    ),
    (class::PALADIN, ROLE_DPS, 1, 20271, cond::TANK_ENGAGED, 0),
];

fn seed_class_role_data(ctx: &ReducerContext) {
    let kits = ctx.db.pkg_playerbots_kit();
    let rotations = ctx.db.pkg_playerbots_rotation();
    for (class, role, priority, spell_id, condition, threshold_pct) in
        DEFAULT_ROTATIONS.iter().copied()
    {
        rotations.insert(PlayerbotsRotation {
            id: 0,
            class,
            role,
            priority,
            spell_id,
            condition,
            threshold_pct,
        });
    }
    for (class, role, spell_id) in default_kit() {
        kits.insert(PlayerbotsKit {
            id: 0,
            class,
            role,
            spell_id,
        });
    }
}

/// The kit derived from [`DEFAULT_ROTATIONS`]: a bot learns exactly what its rotation can cast,
/// deduplicated. Derived rather than typed out a second time, so the two tables cannot drift into
/// a bot that carries a rotation row for a spell it never learned.
pub(crate) fn default_kit() -> Vec<(u8, u8, u32)> {
    let mut kit: Vec<(u8, u8, u32)> = Vec::new();
    for (class, role, _, spell_id, _, _) in DEFAULT_ROTATIONS.iter().copied() {
        if !kit.contains(&(class, role, spell_id)) {
            kit.push((class, role, spell_id));
        }
    }
    kit
}

/// The Refusal for a `(class, role)` this Package has no kit for. The wording is the Operator's
/// only clue, so it names both halves of the pairing.
pub(crate) fn cannot_fill_role_message(class: u8, role: u8) -> String {
    format!("class {class} cannot fill role {role}: no playerbots kit for that pairing")
}

// ---- names -----------------------------------------------------------------------------------

/// The curated name stems a population spawn draws from, in order.
const NAME_STEMS: &[&str] = &[
    "Aldric", "Brenna", "Corwin", "Delia", "Edric", "Fiora", "Garrick", "Halla", "Ivor", "Jessa",
    "Kelen", "Lyra", "Morgan", "Neris", "Orrin", "Perrin", "Quilla", "Rowan", "Sera", "Torvin",
];

/// The stem a role spawn uses, so an Operator who asked for a tank gets a Character it can name.
fn role_name_stem(role: u8) -> &'static str {
    match role {
        ROLE_TANK => "Tankbot",
        ROLE_HEALER => "Healbot",
        _ => "Dpsbot",
    }
}

/// The first free Character name built from `stem`, counting up from 1. Every bot name carries an
/// ordinal — "Tankbot1", "Aldric1" — so one rule mints both the role stems and the curated ones,
/// and a stem that runs out simply keeps counting.
///
/// Gives up after `CHARACTER_NAME_ATTEMPTS` so a realm whose names are genuinely exhausted returns
/// a Refusal instead of looping inside a transaction.
const CHARACTER_NAME_ATTEMPTS: u32 = 512;

fn first_free_name(ctx: &ReducerContext, stem: &str) -> Option<String> {
    let chars = ctx.db.game_character();
    for ordinal in 1..=CHARACTER_NAME_ATTEMPTS {
        let candidate = format!("{stem}{ordinal}");
        if chars.name().find(&candidate).is_none() {
            return Some(candidate);
        }
    }
    None
}

// ---- accounts --------------------------------------------------------------------------------

/// The username of the Package's `index`-th Account block.
pub(crate) fn bot_account_username(index: u32) -> String {
    format!("PLAYERBOT{index:03}")
}

/// Given how many Characters each already-minted bot Account holds, the index of the block a new
/// bot belongs in: the first block with room, or a fresh block past the end. Pure, so the block
/// arithmetic is testable without a live database.
pub(crate) fn account_block_with_room(occupancy: &[usize]) -> u32 {
    occupancy
        .iter()
        .position(|held| *held < CHARACTERS_PER_ACCOUNT)
        .unwrap_or(occupancy.len()) as u32
}

/// The Account the next bot Character goes on, minting a new block when every existing one is
/// full. Bot Accounts carry no credentials: nothing ever logs in to them, and an Account with an
/// empty verifier can complete no SRP handshake, so a minted block is not a way in.
///
/// The occupancy census is one pass over `game_character` that buckets by Account, rather than one
/// indexed count per block: the roster asks about every block at once, and a bot that is mid
/// Transfer counting or not counting toward its block's cap is harmless either way.
fn ensure_bot_account(ctx: &ReducerContext) -> u64 {
    let accounts = ctx.db.game_account();
    let mut blocks: Vec<(u32, u64)> = Vec::new();
    for index in 0.. {
        match accounts.username().find(bot_account_username(index)) {
            Some(account) => blocks.push((index, account.id)),
            None => break,
        }
    }
    let held_by_account: std::collections::HashMap<u64, usize> = ctx
        .db
        .game_character()
        .iter()
        .fold(std::collections::HashMap::new(), |mut counts, character| {
            *counts.entry(character.account_id).or_insert(0) += 1;
            counts
        });
    let occupancy: Vec<usize> = blocks
        .iter()
        .map(|(_, id)| held_by_account.get(id).copied().unwrap_or(0))
        .collect();
    let wanted = account_block_with_room(&occupancy);
    if let Some((_, id)) = blocks.iter().find(|(index, _)| *index == wanted) {
        return *id;
    }
    let username = bot_account_username(wanted);
    accounts.insert(crate::auth::Account {
        id: 0,
        username: username.clone(),
        salt: Vec::new(),
        verifier: Vec::new(),
        identity: None,
        banned: false,
        alpha_test_tools: false,
    });
    accounts
        .username()
        .find(&username)
        .map(|account| account.id)
        .unwrap_or(0)
}

// ---- spawning --------------------------------------------------------------------------------

/// The personality a freshly spawned bot of `role` starts with. The Operator retunes a live bot
/// with a SQL UPDATE; these are only the values it opens with.
pub(crate) fn role_personality_defaults(role: u8) -> (u8, u8) {
    match role {
        // A tank that runs is not a tank.
        ROLE_TANK => (0, 0),
        ROLE_HEALER => (15, 80),
        _ => (15, 0),
    }
}

/// Create one bot Character of `(class, role)`, place it at `at` on `map_id`, teach it its kit, and
/// register it on the roster. Returns the new Character's guid.
fn spawn_one(
    ctx: &ReducerContext,
    class: u8,
    role: u8,
    name_stem: &str,
    map_id: u32,
    at: (f32, f32, f32),
    level: u8,
) -> Result<u64, String> {
    let (x, y, z) = at;
    let name = first_free_name(ctx, name_stem)
        .ok_or_else(|| format!("no free bot name left for stem '{name_stem}'"))?;
    let account_id = ensure_bot_account(ctx);
    crate::auth::create_character(
        ctx,
        account_id,
        name.clone(),
        BOT_RACE,
        class,
        0,
        0,
        0,
        0,
        0,
        0,
    )?;
    // `create_character` reports success, not the guid it minted, so the roster recovers it from
    // the name it just proved free.
    let chars = ctx.db.game_character();
    let mut character = chars
        .name()
        .find(&name)
        .ok_or_else(|| format!("bot character '{name}' vanished after creation"))?;
    let guid = character.guid;

    // The Character was created at its class start position; move it to where the Operator asked
    // for it, and make that its home so a logout-equivalent never drags it back.
    character.map_id = map_id;
    character.x = x;
    character.y = y;
    character.z = z;
    character.home_map = map_id;
    character.home_x = x;
    character.home_y = y;
    character.home_z = z;
    chars.guid().update(character);

    crate::stats::set_character_level(ctx, guid, level as u32)?;

    // `set_character_level` rewrites level, stats and vitals on the durable row, so the copy above
    // is stale. Read the row back and build the live entity from the durable truth.
    let character = chars
        .guid()
        .find(guid)
        .ok_or_else(|| format!("bot character {guid} vanished after levelling"))?;
    let entity = crate::build_player_entity(ctx, &character, Identity::ZERO);
    ctx.db.game_world_entity().insert(entity);

    for spell_id in kit_for(ctx, class, role) {
        crate::spell::learn_spell(ctx, guid, Identity::ZERO, spell_id);
    }

    let (flee_at_pct, heal_at_pct) = role_personality_defaults(role);
    ctx.db
        .pkg_playerbots_personality()
        .insert(PlayerbotsPersonality {
            id: 0,
            character_guid: guid,
            flee_at_pct,
            heal_at_pct,
        });
    ctx.db.pkg_playerbots_bot().insert(PlayerbotsBot {
        id: 0,
        character_guid: guid,
        account_id,
        class,
        role,
        home_map: map_id,
        home_x: x,
        home_y: y,
        home_z: z,
        next_think_micros: 0,
    });
    Ok(guid)
}

/// The kit rows for a `(class, role)` pair, as spell ids.
pub(crate) fn kit_for(ctx: &ReducerContext, class: u8, role: u8) -> Vec<u32> {
    ctx.db
        .pkg_playerbots_kit()
        .by_class_role()
        .filter((class, role))
        .map(|row| row.spell_id)
        .collect()
}

/// Can this Package field a `(class, role)` bot? The kit table is the answer, so a live SQL insert
/// that adds a kit for a new pairing makes that pairing legal with no republish.
fn can_fill_role(ctx: &ReducerContext, class: u8, role: u8) -> bool {
    ctx.db
        .pkg_playerbots_kit()
        .by_class_role()
        .filter((class, role))
        .next()
        .is_some()
}

fn default_class_for_role(role: u8) -> Option<u8> {
    DEFAULT_CLASS_FOR_ROLE
        .iter()
        .find(|(r, _)| *r == role)
        .map(|(_, class)| *class)
}

/// The map the Operator's coordinates belong to. Every spawn verb takes a point and no map,
/// because a Package reducer runs on one Shard and the Shard's own world map is the only map those
/// coordinates can mean.
const SPAWN_MAP_ID: u32 = 0;

fn spawn_batch(
    ctx: &ReducerContext,
    count: u32,
    at: (f32, f32, f32),
    class: u8,
    role: u8,
    name_stem: &str,
) -> Result<(), String> {
    if !can_fill_role(ctx, class, role) {
        return Err(cannot_fill_role_message(class, role));
    }
    let level = spawn_level(ctx);
    for _ in 0..count {
        spawn_one(ctx, class, role, name_stem, SPAWN_MAP_ID, at, level)?;
    }
    Ok(())
}

/// The level a spawned bot is created at: the midpoint of the configured band, clamped so a
/// reversed or absurd band still produces a legal Character level.
fn spawn_level(ctx: &ReducerContext) -> u8 {
    let min = config_parsed::<u8>(ctx, "level_min", 1).max(1);
    let max = config_parsed::<u8>(ctx, "level_max", 10).max(min);
    min + (max - min) / 2
}

// ---- Operator verbs ---------------------------------------------------------------------------

/// Spawn `count` bots at `(x, y, z)`, cycling the roles so the batch forms a working party.
#[reducer]
pub fn playerbots_spawn(
    ctx: &ReducerContext,
    count: u32,
    x: f32,
    y: f32,
    z: f32,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    ensure_defaults(ctx);
    let level = spawn_level(ctx);
    let stems = NAME_STEMS;
    let roster = ctx.db.pkg_playerbots_bot().count() as usize;
    for index in 0..count as usize {
        let role = DEFAULT_CLASS_FOR_ROLE[(roster + index) % DEFAULT_CLASS_FOR_ROLE.len()].0;
        let class =
            default_class_for_role(role).ok_or_else(|| cannot_fill_role_message(0, role))?;
        if !can_fill_role(ctx, class, role) {
            return Err(cannot_fill_role_message(class, role));
        }
        let stem = stems[(roster + index) % stems.len()];
        spawn_one(ctx, class, role, stem, SPAWN_MAP_ID, (x, y, z), level)?;
    }
    Ok(())
}

/// Spawn `count` bots of one role, using this Package's default class for that role.
#[reducer]
pub fn playerbots_spawn_role(
    ctx: &ReducerContext,
    count: u32,
    x: f32,
    y: f32,
    z: f32,
    role: u8,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    ensure_defaults(ctx);
    let class = default_class_for_role(role).ok_or_else(|| cannot_fill_role_message(0, role))?;
    spawn_batch(ctx, count, (x, y, z), class, role, role_name_stem(role))
}

/// Spawn `count` bots of one `(class, role)` pairing. Refuses a pairing this Package has no kit
/// for, by name, rather than creating a Character that would stand there with nothing to cast.
#[reducer]
pub fn playerbots_spawn_class_role(
    ctx: &ReducerContext,
    count: u32,
    x: f32,
    y: f32,
    z: f32,
    class: u8,
    role: u8,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    ensure_defaults(ctx);
    spawn_batch(ctx, count, (x, y, z), class, role, role_name_stem(role))
}

/// Remove every bot: the Character and everything it owns, through the same cascade a real
/// Character deletion uses. The roster row and the personality row go with it through their own
/// sweeps, so there is one delete path and nothing to keep in step with it.
#[reducer]
pub fn playerbots_despawn_all(ctx: &ReducerContext) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let guids: Vec<u64> = ctx
        .db
        .pkg_playerbots_bot()
        .iter()
        .map(|bot| bot.character_guid)
        .collect();
    for guid in guids {
        // A bot in a party must leave it first: the cascade deletes the Character, and a member row
        // pointing at a Character that no longer exists is a party the survivors cannot dissolve.
        let _ = crate::group::leave_group_for(ctx, guid);
        crate::world::cascade_delete_character(ctx, guid);
    }
    Ok(())
}

/// How many bots a top-up must create to reach `target`. Pure, so idempotence is a property of a
/// function rather than of a live run: at or above target it is zero, and calling it again after a
/// successful top-up is zero again.
pub(crate) fn populate_shortfall(existing: usize, target: usize) -> usize {
    target.saturating_sub(existing)
}

/// Top the population up to the configured `population_count` at the configured home point.
/// Idempotent: a second call with nothing changed creates nothing. Nothing calls this on install or
/// on publish — a realm gets bots when its Operator asks for them.
#[reducer]
pub fn playerbots_populate(ctx: &ReducerContext) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    ensure_defaults(ctx);
    let target = config_parsed::<usize>(ctx, "population_count", 0);
    let existing = ctx.db.pkg_playerbots_bot().count() as usize;
    let shortfall = populate_shortfall(existing, target);
    if shortfall == 0 {
        return Ok(());
    }
    let (x, y, z) = home_point(ctx);
    playerbots_spawn(ctx, shortfall as u32, x, y, z)
}

/// The configured home point every populated bot spawns at and every ungrouped bot returns to.
pub(crate) fn home_point(ctx: &ReducerContext) -> (f32, f32, f32) {
    (
        config_parsed::<f32>(ctx, "home_x", -8930.0),
        config_parsed::<f32>(ctx, "home_y", -250.0),
        config_parsed::<f32>(ctx, "home_z", 80.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rotation_spell_is_in_its_own_class_role_kit() {
        let kit = default_kit();
        for (class, role, _, spell_id, _, _) in DEFAULT_ROTATIONS.iter().copied() {
            assert!(
                kit.contains(&(class, role, spell_id)),
                "rotation spell {spell_id} for class {class} role {role} is not in that pair's kit, \
                 so the bot would carry a rotation row for a spell it never learned"
            );
        }
    }

    #[test]
    fn the_kit_holds_no_spell_its_rotation_never_casts() {
        for (class, role, spell_id) in default_kit() {
            assert!(
                DEFAULT_ROTATIONS
                    .iter()
                    .any(|(c, r, _, s, _, _)| *c == class && *r == role && *s == spell_id),
                "kit spell {spell_id} for class {class} role {role} appears in no rotation row"
            );
        }
    }

    #[test]
    fn one_class_fills_three_roles_with_three_different_rotations() {
        let paladin: Vec<(u8, u32)> = DEFAULT_ROTATIONS
            .iter()
            .filter(|(class, ..)| *class == class::PALADIN)
            .map(|(_, role, _, spell_id, _, _)| (*role, *spell_id))
            .collect();
        for role in [ROLE_TANK, ROLE_HEALER, ROLE_DPS] {
            let spells: Vec<u32> = paladin
                .iter()
                .filter(|(r, _)| *r == role)
                .map(|(_, s)| *s)
                .collect();
            assert!(
                !spells.is_empty(),
                "the Paladin has no rotation for role {role}"
            );
        }
        let tank: Vec<u32> = paladin
            .iter()
            .filter(|(r, _)| *r == ROLE_TANK)
            .map(|(_, s)| *s)
            .collect();
        let healer: Vec<u32> = paladin
            .iter()
            .filter(|(r, _)| *r == ROLE_HEALER)
            .map(|(_, s)| *s)
            .collect();
        assert_ne!(
            tank, healer,
            "the Paladin tank and healer must not share one rotation"
        );
    }

    #[test]
    fn an_unkitted_pairing_is_not_fillable() {
        let pairs: Vec<(u8, u8)> = default_kit()
            .into_iter()
            .map(|(class, role, _)| (class, role))
            .collect();
        assert!(pairs.contains(&(class::PALADIN, ROLE_TANK)));
        assert!(
            !pairs.contains(&(class::MAGE, ROLE_TANK)),
            "a Mage cannot tank, so no kit may exist for that pairing"
        );
    }

    #[test]
    fn the_role_refusal_names_both_halves_of_the_pairing() {
        let message = cannot_fill_role_message(class::MAGE, ROLE_TANK);
        assert!(message.contains("cannot fill role"), "{message}");
        assert!(message.contains("8"), "{message}");
        assert!(message.contains("0"), "{message}");
    }

    #[test]
    fn a_top_up_creates_only_the_shortfall() {
        assert_eq!(populate_shortfall(0, 10), 10);
        assert_eq!(populate_shortfall(4, 10), 6);
    }

    #[test]
    fn a_top_up_at_or_over_target_creates_nothing() {
        assert_eq!(populate_shortfall(10, 10), 0);
        assert_eq!(populate_shortfall(12, 10), 0);
    }

    #[test]
    fn the_first_account_block_takes_the_first_ten_bots() {
        assert_eq!(account_block_with_room(&[]), 0);
        assert_eq!(account_block_with_room(&[9]), 0);
    }

    #[test]
    fn a_full_account_block_mints_the_next_one() {
        assert_eq!(account_block_with_room(&[10]), 1);
        assert_eq!(account_block_with_room(&[10, 10]), 2);
    }

    #[test]
    fn a_block_that_freed_a_slot_is_refilled_before_a_new_one_is_minted() {
        assert_eq!(
            account_block_with_room(&[10, 9, 10]),
            1,
            "a despawn frees a slot; the roster must reuse it rather than minting Accounts forever"
        );
    }

    #[test]
    fn account_block_usernames_sort_in_block_order() {
        assert_eq!(bot_account_username(0), "PLAYERBOT000");
        assert_eq!(bot_account_username(12), "PLAYERBOT012");
        assert!(bot_account_username(2) < bot_account_username(10));
    }

    #[test]
    fn a_tank_never_flees_and_a_healer_opens_at_eighty() {
        assert_eq!(role_personality_defaults(ROLE_TANK), (0, 0));
        assert_eq!(role_personality_defaults(ROLE_HEALER), (15, 80));
        assert_eq!(role_personality_defaults(ROLE_DPS).0, 15);
    }

    #[test]
    fn every_config_key_carries_a_default_this_package_can_parse() {
        for (key, value) in CONFIG_DEFAULTS {
            match key {
                "population_count" => assert!(value.parse::<usize>().is_ok(), "{key}"),
                "level_min" | "level_max" => assert!(value.parse::<u8>().is_ok(), "{key}"),
                _ => assert!(value.parse::<f32>().is_ok(), "{key}"),
            }
        }
    }
}
