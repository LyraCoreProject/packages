# T3: reconcile the Playerbots Loot Tag path and verify the issue

Parent: issue #15. **Runs last after T1 and T2 are integrated.**
Model: GPT-5.6 Sol, xhigh. Estimated size: ~150k tokens.

## Problem

T1 and T2 change adjacent parts of one brain loop. Each slice can pass alone while their union still
retries a refused corpse, records progress for a skipped target, filters a friendly rotation target
or grows a second ownership rule. The final branch also needs evidence for the quest-credit story,
which is intentionally implemented by LyraCore rather than this Package.

## Delivery

Read the combined `playerbots/src/goals.rs` diff and trace one full ungrouped brain tick and one
grouped combat tick. Reconcile duplicated helpers or inconsistent names. Keep one live target
policy, one corpse eligibility decision and one stable Refusal classifier.

Check the ownership boundary with searches and code reading:

- Playerbots must not query `game_creature_quest_tap`, `game_creature_quest_tap_member`,
  `game_creature_loot_tag_group` or `game_corpse_loot_eligible`.
- Playerbots must not call `record_first_threat`, `on_creature_killed` or `kill_creature`.
- Live selection must use `death_entitlement`.
- Corpse selection must use `corpse_eligible_recipients` and
  `corpse_eligible_for_access`.
- Money and item changes must still pass through their existing actor functions.

Confirm the quest-state path instead of adding Package kill accounting. A bot that remains entitled
at death receives credit from Module death dispatch even when another Character lands the lethal
hit. A bot absent from the entitlement does not. On the next brain tick, Playerbots reads the
resulting `CharacterQuest.counts` and `quest_is_complete`. Keep `QuestWork::Progress` as a stall-clock
signal for active quest combat only; do not turn it into kill credit.

Fix any in-scope defect found during this review and add a regression test with the fix. Do not
rewrite a working slice for taste.

## Acceptance criteria

1. The combined branch satisfies every checkbox on packages issue #15.
2. A foreign tagged live creature is skipped, while an untagged or entitled creature remains
   available.
3. A foreign corpse causes no normal loot attempt. A raced ownership Refusal ends work on that
   corpse without retrying its remaining operations.
4. A foreign killing blow alone never changes Package quest state. Module entitlement is the only
   source of kill credit, and Playerbots follows the resulting durable quest row.
5. No Package-owned copy of Loot Tag membership, range, corpse or quest-credit state exists.
6. The README and focused tests describe the code that ships.

## Verification

1. Run all focused Playerbots tests introduced by T1 and T2.
2. Run the required compatibility suite:

   ```bash
   ./.github/check-core-tip.sh /path/to/clean/LyraCore-at-7c96b65
   ```

3. Run the merged core's ignored standalone Loot Tag fixture with `debug_reducers`. It already
   covers a stranger lethal hit, entitled quest credit, an unentitled Character, corpse eligibility
   and the stable Refusal. If local SpacetimeDB or wasm tooling blocks it, report the exact tool
   failure and compare any source-only check with the clean core commit.
4. Run whitespace checks on the Package diff and confirm no unrelated formatting churn.
5. Review the final diff against `CODING_STANDARDS.md` and this shared brief.

## Non-goals

- Do not add a new core API unless the merged API is proved unusable. Report that as a blocker
  before expanding scope.
- Do not change Gateway behavior or client-visible Loot Tag flags.
- Do not add Package kill-credit, corpse-target or party-snapshot state.
- Do not touch a realm or live database.

## Definition of done

The combined branch is rebased on current packages `main`, required tests pass or have a documented
clean-base tool failure, and the final diff contains no ownership logic outside the Module contract.
