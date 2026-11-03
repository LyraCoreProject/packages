//! Bots buy and wear better gear from a vendor near them while no fight is on.

use super::decision::{Action, ActionNode, MoveTarget, Purchase, Reason};
use super::{class, PlayerbotsBot, ROLE_HEALER, ROLE_TANK};
use crate::tables::{
    game_item_enchantment, game_item_instance, game_item_random_property, game_item_template,
    game_npc_vendor, game_world_entity, ItemTemplate, WorldEntity,
};
use lyracore_shared::item_property as stat;
use spacetimedb::ReducerContext;

const SHOP_RADIUS_YD: f32 = 30.0;
// The core's interaction range is 10 yards; a bot stops a little inside it.
const VENDOR_REACH_YD: f32 = 8.0;
const VENDOR_LIMIT: usize = 4;
const STOCK_LIMIT: usize = 64;
const PRIORITY: i32 = 150;
// A smaller gain is noise from rounding, and buying on it would churn gold.
const MIN_GAIN: f32 = 1.0;
const ITEM_CLASS_WEAPON: u8 = 2;
const ITEM_CLASS_ARMOR: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Profile {
    Tank,
    Healer,
    Caster,
    Melee,
}

fn profile(bot: &PlayerbotsBot) -> Profile {
    match bot.role {
        ROLE_TANK => Profile::Tank,
        ROLE_HEALER => Profile::Healer,
        _ if matches!(bot.class, class::MAGE | class::PRIEST) => Profile::Caster,
        _ => Profile::Melee,
    }
}

// Points per unit of a stat. Crit and hit are per percent.
fn weight(profile: Profile, kind: u8) -> f32 {
    use Profile::*;
    match (profile, kind) {
        (Tank, stat::STAMINA | stat::DEFENSE | stat::DODGE | stat::PARRY | stat::BLOCK) => 1.0,
        (Tank, stat::STRENGTH | stat::AGILITY) => 0.5,
        (Tank, stat::ARMOR) => 0.15,
        (Healer, stat::INTELLECT | stat::HEALING_POWER | stat::MANA_PER_FIVE) => 1.0,
        (Healer, stat::SPIRIT) => 0.8,
        (Caster, stat::INTELLECT | stat::SPELL_POWER) => 1.0,
        (Caster, stat::SPIRIT) => 0.5,
        (Melee, stat::STRENGTH) => 1.0,
        (Melee, stat::AGILITY | stat::CRIT | stat::HIT) => 0.7,
        (Healer | Caster | Melee, stat::STAMINA) => 0.4,
        (_, stat::ARMOR) => 0.05,
        _ => 0.0,
    }
}

fn weapon_weight(profile: Profile) -> f32 {
    match profile {
        Profile::Melee => 2.0,
        Profile::Tank => 1.0,
        Profile::Healer | Profile::Caster => 0.0,
    }
}

const SCORED: [u8; 16] = [
    stat::STRENGTH,
    stat::AGILITY,
    stat::STAMINA,
    stat::INTELLECT,
    stat::SPIRIT,
    stat::ARMOR,
    stat::SPELL_POWER,
    stat::HEALING_POWER,
    stat::MANA_PER_FIVE,
    stat::CRIT,
    stat::HIT,
    stat::DEFENSE,
    stat::DODGE,
    stat::PARRY,
    stat::BLOCK,
    stat::WEAPON_DAMAGE,
];

fn template_stat(template: &ItemTemplate, kind: u8) -> f32 {
    match kind {
        stat::STRENGTH => template.stat_strength as f32,
        stat::AGILITY => template.stat_agility as f32,
        stat::STAMINA => template.stat_stamina as f32,
        stat::INTELLECT => template.stat_intellect as f32,
        stat::SPIRIT => template.stat_spirit as f32,
        stat::ARMOR => template.stat_armor as f32,
        // The template stores crit and hit in attack-table basis points.
        stat::CRIT => template.stat_crit as f32 / 100.0,
        stat::HIT => template.stat_hit as f32 / 100.0,
        stat::BLOCK => template.block_value as f32,
        _ => 0.0,
    }
}

fn property_stat(ctx: &ReducerContext, property_id: u32, kind: u8) -> f32 {
    let Some(property) = ctx
        .db
        .game_item_random_property()
        .property_id()
        .find(property_id)
    else {
        return 0.0;
    };
    stat::property_stat(
        [
            property.enchant_id_1,
            property.enchant_id_2,
            property.enchant_id_3,
        ],
        |enchant| {
            stat::enchant_stat(
                kind,
                u32::MAX,
                ctx.db
                    .game_item_enchantment()
                    .by_enchant()
                    .filter(enchant)
                    .map(|row| (row.kind, row.amount, row.school_mask)),
            )
        },
    ) as f32
}

fn score(ctx: &ReducerContext, profile: Profile, template: &ItemTemplate, property: u32) -> f32 {
    let stats: f32 = SCORED
        .iter()
        .map(|&kind| {
            let rolled = if property == 0 {
                0.0
            } else {
                property_stat(ctx, property, kind)
            };
            weight(profile, kind) * (template_stat(template, kind) + rolled)
        })
        .sum();
    let dps = if template.delay_ms > 0 {
        (template.damage_min + template.damage_max) / 2.0 * 1000.0 / template.delay_ms as f32
    } else {
        0.0
    };
    stats + weapon_weight(profile) * dps
}

// Mirrors core `inventory_type_to_slot`; a ring, trinket or two-hander names both its slots.
fn slots_for(inventory_type: u8) -> &'static [u8] {
    match inventory_type {
        1 => &[0],
        2 => &[1],
        3 => &[2],
        5 | 20 => &[4],
        6 => &[5],
        7 => &[6],
        8 => &[7],
        9 => &[8],
        10 => &[9],
        11 => &[10, 11],
        12 => &[12, 13],
        13 | 21 => &[15],
        14 | 22 | 23 => &[16],
        15 | 25 | 26 => &[17],
        16 => &[14],
        17 => &[15, 16],
        _ => &[],
    }
}

fn equipped_score(ctx: &ReducerContext, profile: Profile, guid: u64, slot: u8) -> Option<f32> {
    let item = ctx
        .db
        .game_item_instance()
        .by_owner_guid()
        .filter(guid)
        .find(|item| item.slot == slot)?;
    let template = ctx.db.game_item_template().entry().find(item.entry)?;
    Some(score(ctx, profile, &template, item.random_property_id))
}

/// What the item would replace: a two-hander both hands, a ring or trinket only an empty slot.
fn replaced_score(
    ctx: &ReducerContext,
    profile: Profile,
    guid: u64,
    inventory_type: u8,
) -> Option<f32> {
    let slots = slots_for(inventory_type);
    let scores: Vec<_> = slots
        .iter()
        .map(|&slot| equipped_score(ctx, profile, guid, slot))
        .collect();
    match (inventory_type, scores.as_slice()) {
        (17, [main, off]) => Some(main.unwrap_or(0.0) + off.unwrap_or(0.0)),
        (_, [Some(_), Some(_)]) => None,
        (_, [_, _]) => Some(0.0),
        (_, [one]) => Some(one.unwrap_or(0.0)),
        _ => None,
    }
}

fn wearable(ctx: &ReducerContext, me: &WorldEntity, template: &ItemTemplate) -> bool {
    let proficiency = lyracore_shared::item::Proficiency::from_spellbook(me.class(), |spell| {
        crate::actor::knows_spell(ctx, me.guid, spell)
    });
    matches!(template.class, ITEM_CLASS_WEAPON | ITEM_CLASS_ARMOR)
        && u32::from(template.required_level) <= me.level
        && proficiency.can_equip(template.class, template.subclass)
}

fn carries(ctx: &ReducerContext, guid: u64, entry: u32) -> bool {
    ctx.db
        .game_item_instance()
        .by_owner_guid()
        .filter(guid)
        .any(|item| item.entry == entry)
}

/// The best affordable upgrade a nearby vendor stocks for this bot, and that vendor.
fn best_purchase(ctx: &ReducerContext, bot: &PlayerbotsBot, me: &WorldEntity) -> Option<Purchase> {
    let profile = profile(bot);
    let vendors =
        crate::actor::entities_near(ctx, me.map_id, me.instance_id, me.x, me.y, SHOP_RADIUS_YD)
            .into_iter()
            .filter(|npc| {
                !npc.is_player()
                    && !npc.dead
                    && npc.npc_flags & lyracore_shared::constants::npc_flags::VENDOR != 0
            })
            .take(VENDOR_LIMIT);
    let mut best: Option<(f32, u32, Purchase)> = None;
    for vendor in vendors {
        for offer in ctx
            .db
            .game_npc_vendor()
            .by_vendor()
            .filter(vendor.entry)
            .take(STOCK_LIMIT)
        {
            let Some(template) = ctx.db.game_item_template().entry().find(offer.item_entry) else {
                continue;
            };
            if template.buy_price == 0
                || template.buy_price > me.money
                || !wearable(ctx, me, &template)
                || carries(ctx, me.guid, template.entry)
            {
                continue;
            }
            let Some(current) = replaced_score(ctx, profile, me.guid, template.inventory_type)
            else {
                continue;
            };
            let gain = score(ctx, profile, &template, 0) - current;
            let better = best.as_ref().is_none_or(|(best_gain, price, _)| {
                gain > *best_gain || (gain == *best_gain && template.buy_price < *price)
            });
            if gain >= MIN_GAIN && better {
                let purchase = Purchase {
                    vendor: vendor.guid,
                    item: template.entry,
                };
                best = Some((gain, template.buy_price, purchase));
            }
        }
    }
    best.map(|(_, _, purchase)| purchase)
}

pub(super) fn candidate(
    ctx: &ReducerContext,
    bot: &PlayerbotsBot,
    me: &WorldEntity,
    objective: u64,
) -> Option<ActionNode> {
    let purchase = best_purchase(ctx, bot, me)?;
    let mut buy = ActionNode::ready(Action::Buy(purchase), Reason::Shopping, PRIORITY);
    buy.candidate.id.objective = objective;
    let vendor = ctx.db.game_world_entity().guid().find(purchase.vendor)?;
    if (me.x - vendor.x).powi(2) + (me.y - vendor.y).powi(2) > VENDOR_REACH_YD.powi(2) {
        let mut approach = ActionNode::ready(
            Action::Move(MoveTarget::Entity(purchase.vendor)),
            Reason::Shopping,
            PRIORITY,
        );
        approach.candidate.id.objective = objective;
        buy.prerequisites.push(approach);
    }
    Some(buy)
}

/// Buys the item, then wears it; a refused equip leaves it in the bags.
pub(super) fn execute(ctx: &ReducerContext, guid: u64, purchase: Purchase) -> Result<(), String> {
    crate::actor::buy_item(ctx, guid, purchase.vendor, purchase.item, 1).map_err(String::from)?;
    let slot = ctx
        .db
        .game_item_instance()
        .by_owner_guid()
        .filter(guid)
        .filter(|item| {
            item.entry == purchase.item && lyracore_shared::item::is_carried_slot(item.slot)
        })
        .map(|item| item.slot)
        .min()
        .ok_or("the bought item is not in the bags")?;
    crate::actor::equip_item(ctx, guid, slot).map_err(|refusal| format!("{refusal:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_profile_values_its_own_stats_most() {
        assert!(weight(Profile::Tank, stat::STAMINA) > weight(Profile::Tank, stat::INTELLECT));
        assert!(weight(Profile::Healer, stat::INTELLECT) > weight(Profile::Healer, stat::STRENGTH));
        assert!(weight(Profile::Caster, stat::INTELLECT) > weight(Profile::Caster, stat::AGILITY));
        assert!(weight(Profile::Melee, stat::STRENGTH) > weight(Profile::Melee, stat::INTELLECT));
        assert_eq!(weapon_weight(Profile::Caster), 0.0);
    }

    #[test]
    fn rings_and_two_handers_name_every_slot_they_touch() {
        assert_eq!(slots_for(11), &[10, 11]);
        assert_eq!(slots_for(17), &[15, 16]);
        assert_eq!(slots_for(20), slots_for(5));
        assert!(slots_for(18).is_empty(), "a bag is not gear");
    }
}
