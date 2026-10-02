//! Rank selection for the spell families named by a kit or rotation.

use crate::game_spell_chain;
use spacetimedb::ReducerContext;

const FAMILY_LIMIT: usize = 16;

/// Read bounded rank data in order. Core Gates decide whether prerequisites are satisfied.
/// A spell without imported rank metadata remains a single-spell family.
pub(super) fn family(ctx: &ReducerContext, spell: u32) -> Result<Vec<u32>, String> {
    let Some(root) = ctx.db.game_spell_chain().spell_id().find(spell) else {
        return Ok(vec![spell]);
    };
    let mut rows: Vec<_> = ctx
        .db
        .game_spell_chain()
        .by_first()
        .filter(root.first_spell)
        .take(FAMILY_LIMIT + 1)
        .collect();
    if rows.len() > FAMILY_LIMIT {
        return Err(format!(
            "spell family {} exceeds {FAMILY_LIMIT} ranks",
            root.first_spell
        ));
    }
    rows.sort_by_key(|row| (row.rank, row.spell_id));
    Ok(rows.into_iter().map(|row| row.spell_id).collect())
}

pub(super) fn highest_known(ctx: &ReducerContext, guid: u64, spell: u32) -> Result<u32, String> {
    Ok(family(ctx, spell)?
        .into_iter()
        .rev()
        .find(|rank| crate::spell::knows_spell(ctx, guid, *rank))
        .unwrap_or(spell))
}
