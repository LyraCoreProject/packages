#!/usr/bin/env bash
set -euo pipefail

collection_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
core_root=${1:?usage: check-playerbots-target-claims.sh /path/to/LyraCore}
core_root=$(cd "$core_root" && pwd)
"$collection_root/.github/check-playerbots.sh" "$core_root" test --locked \
    --test playerbots_target_claims -- --ignored --nocapture --test-threads=1

"$collection_root/.github/check-playerbots.sh" "$core_root" test --locked --test playerbots_quest_loops \
    playerbots_quest_fallback_distributes_an_ungrouped_population \
    -- --ignored --exact --nocapture --test-threads=1

"$collection_root/.github/check-playerbots.sh" "$core_root" test --locked --test playerbots_quest_catalog \
    playerbots_quest_objective_survives_combat_and_refreshes_changed_evidence \
    -- --ignored --exact --nocapture --test-threads=1

"$collection_root/.github/check-playerbots.sh" "$core_root" test --locked --test playerbots_recovery \
    playerbots_recovery_changes_a_stalled_attack_then_defers_without_false_progress \
    -- --ignored --exact --nocapture --test-threads=1

"$collection_root/.github/check-playerbots.sh" "$core_root" test --locked --test playerbots_recovery \
    playerbots_recovery_defers_a_moving_leader_and_allows_a_real_self_heal \
    -- --ignored --exact --nocapture --test-threads=1
