#![cfg(feature = "debug_reducers")]

use spacetimedb::{reducer, ReducerContext};

#[reducer]
pub fn wave_defense_content_fixture_stock(ctx: &ReducerContext) -> Result<(), String> {
    crate::package::require_operator(ctx)?;
    crate::package::fixture::require_no_imported_content(ctx)?;
    super::content::stock_quartermaster(ctx)
}
