use spacetimedb::{table, SpacetimeType};

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Preparation,
    Combat,
    Won,
    Lost,
}

impl Phase {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Preparation => "preparation",
            Self::Combat => "combat",
            Self::Won => "won",
            Self::Lost => "lost",
        }
    }
}

#[table(accessor = pkg_wave_defense_config)]
pub struct DefenseConfig {
    #[primary_key]
    pub id: u8,
    pub enabled: bool,
    pub lobby_instance_id: u64,
    pub map_id: u32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub orientation: f32,
    pub spawn_x: f32,
    pub spawn_y: f32,
    pub spawn_z: f32,
    pub creature_entry: u32,
    pub boss_entry: u32,
    pub vendor_entry: u32,
    pub pet_entry: u32,
}

#[table(accessor = pkg_wave_defense_profile, index(accessor = by_lobby_pending, btree(columns = [lobby_pending])))]
pub struct DefenseProfile {
    #[primary_key]
    pub character_guid: u64,
    pub class: u8,
    pub selected: bool,
    pub last_request: u64,
    pub lobby_pending: bool,
}

crate::character_owned!(delete, fn sweep_delete_pkg_wave_defense_profile(ctx, character_guid) {
    ctx.db.pkg_wave_defense_profile().character_guid().delete(character_guid);
});
crate::character_owned!(not_transported, fn sweep_transfer_pkg_wave_defense_profile());

#[table(accessor = pkg_wave_defense_queue, index(accessor = by_group, btree(columns = [group_id])))]
pub struct DefenseQueue {
    #[primary_key]
    pub character_guid: u64,
    pub group_id: u64,
    pub queued_micros: i64,
}

crate::character_owned!(delete, fn sweep_delete_pkg_wave_defense_queue(ctx, character_guid) {
    ctx.db.pkg_wave_defense_queue().character_guid().delete(character_guid);
});
crate::character_owned!(not_transported, fn sweep_transfer_pkg_wave_defense_queue());

#[table(accessor = pkg_wave_defense_match)]
pub struct DefenseMatch {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub instance_id: u64,
    pub group_id: u64,
    pub phase: Phase,
    pub wave: u8,
    pub level: u8,
    pub team_xp: u32,
    pub base_health: u32,
    pub deadline_micros: i64,
    pub wave_started_micros: i64,
    pub next_spawn_micros: i64,
    pub spawned: u16,
    pub defeated: u16,
    pub next_base_hit_micros: i64,
    pub empty_human_micros: i64,
}

#[table(accessor = pkg_wave_defense_member, index(accessor = by_match, btree(columns = [match_id])))]
pub struct DefenseMember {
    #[primary_key]
    pub character_guid: u64,
    pub match_id: u64,
    pub account_id: u64,
    pub class: u8,
    pub bot: bool,
    pub replacement_for: u64,
    pub disconnected_micros: i64,
    pub resume_requested: bool,
    pub respawn_micros: i64,
    pub ready: bool,
    pub choices_spent: u8,
    pub offer_revision: u32,
    pub offer_ids: Vec<u32>,
    pub acquired: Vec<u32>,
}

impl DefenseMember {
    pub(crate) fn active(&self) -> bool {
        self.bot || self.disconnected_micros == 0
    }
}

crate::character_owned!(delete, fn sweep_delete_pkg_wave_defense_member(ctx, character_guid) {
    ctx.db.pkg_wave_defense_member().character_guid().delete(character_guid);
});
crate::character_owned!(not_transported, fn sweep_transfer_pkg_wave_defense_member());

#[table(accessor = pkg_wave_defense_enemy, index(accessor = by_match, btree(columns = [match_id])))]
pub struct DefenseEnemy {
    #[primary_key]
    pub creature_guid: u64,
    pub match_id: u64,
    pub boss: bool,
    pub moving_until_micros: i64,
    pub next_ability_micros: i64,
    pub ability_count: u16,
}

#[table(accessor = pkg_wave_defense_progress)]
pub struct DefenseProgress {
    #[primary_key]
    pub account_id: u64,
    pub waves: u32,
    pub bosses: u32,
    pub wins: u32,
}
