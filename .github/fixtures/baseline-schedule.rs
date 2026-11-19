#[cfg(feature = "debug_reducers")]
mod package_schedule_fixture {
    use crate::game_creature_move_schedule;
    use spacetimedb::{reducer, ReducerContext, Table};

    #[reducer]
    pub fn debug_clear_schedule(ctx: &ReducerContext, table: String) -> Result<(), String> {
        crate::helpers::require_operator(ctx)?;
        if table != "game_creature_move_schedule" {
            return Err(format!("{table} is not a clearable baseline schedule"));
        }
        let schedules = ctx.db.game_creature_move_schedule();
        let rows: Vec<_> = schedules.iter().collect();
        for row in rows {
            schedules.delete(row);
        }
        Ok(())
    }
}
