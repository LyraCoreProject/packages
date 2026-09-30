#[allow(dead_code)] // Test targets select the evidence and historical fixtures they need.
pub mod evidence;
#[allow(dead_code)]
pub mod pb002;

pub use lyracore_test_support::*;

/// Stage an active class buff before a test starts a movement, quest, or lifecycle decision.
/// Preserve the staged power budget. Buff selection and cooldown behavior have separate role tests.
#[allow(dead_code)]
pub fn stage_playerbot_buff(node: &Standalone, guid: &str) {
    let bot = node.query_rows(&format!(
        "SELECT class FROM pkg_playerbots_bot WHERE character_guid = {guid}"
    ));
    let spell = match bot[0]["class"].as_str() {
        "1" => "6673",
        "5" => "1243",
        "8" => "168",
        _ => return,
    };
    let power = node.query_rows(&format!(
        "SELECT power FROM game_world_entity WHERE guid = {guid}"
    ))[0]["power"]
        .clone();
    node.assert_sql(&format!(
        "UPDATE game_spell SET duration_ms = 3600000 WHERE spell_id = {spell}"
    ));
    node.assert_sql(&format!(
        "DELETE FROM game_spell_cooldown WHERE caster_guid = {guid}"
    ));
    node.assert_call("debug_force_cast", &[guid, spell]);
    node.assert_call("debug_set_power", &[guid, &power]);
    node.assert_sql(&format!(
        "DELETE FROM game_spell_cooldown WHERE caster_guid = {guid}"
    ));
    assert!(!node
        .query_rows(&format!(
            "SELECT spell_id FROM game_aura WHERE target_guid = {guid} AND spell_id = {spell}"
        ))
        .is_empty());
}
