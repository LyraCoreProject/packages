# T1: filter live targets through Module Loot Tag entitlement

Parent: issue #15. **Tracer bullet. Runs alone and blocks T2 and T3.**
Model: GPT-5.6 Sol, xhigh. Estimated size: ~170k tokens.

## Problem

Playerbots chooses combat targets from an existing melee row, a creature attacking the party, a
quest kill objective, the grind scan and rotation-specific assist helpers. None consults Loot Tag
ownership. A bot can therefore adopt a creature tagged by a foreign party, spend a full Engagement
on it, then receive no reward or quest credit.

The Package has no business interpreting `game_creature_quest_tap`, its tag-time member rows or
current party membership. LyraCore already computes the recipient set in `death_entitlement`.

## Delivery

Establish one small Playerbots policy seam in `playerbots/src/goals.rs` for a live target:

```text
untagged creature                    -> available
tagged and bot Character recipient   -> available
tagged and bot Character absent      -> foreign, skip
```

Feed the target entity into `crate::loot::death_entitlement` with its position, map and instance.
Do not read live Loot Tag tables in the Package. Keep a pure recipient decision underneath the
context-facing helper so the policy has focused tests without a fake `ReducerContext`.

Apply the seam at the narrowest useful points so every deliberate hostile target passes through
it:

- the existing melee target returned by `combat_target`;
- party-assist candidates in `combat_target`;
- quest targets selected by `work_quest`;
- grind targets selected by `grind`;
- hostile targets selected by rotation helpers, including peel and tank-assist targets.

If the bot's existing melee row points at a creature that has become foreign, request
`actor::stop_attack` and let the brain choose another goal. Do not let `engaged_reason` report quest
progress for a target that the policy refused.

Keep the immediate `on_damage_taken` hook as a survival response. It may start a defensive swing,
but the next regular brain tick must release a foreign tagged creature. Do not suppress heals,
self-casts or ally buffs when filtering rotation targets.

Keep `pick_near` generic and pure. The Loot Tag read belongs in a caller predicate or in the new
policy seam, not inside the generic nearest-three selector.

## Acceptance criteria

1. An untagged wild creature remains available for quest hunting, grinding and party assist.
2. A tagged creature remains available when `death_entitlement.recipients` contains the bot
   Character.
3. A tagged creature is absent from quest, grind and party-assist choices when the bot Character is
   not a recipient.
4. A stale current-melee row for a foreign tagged creature is stopped and is not returned as the
   current goal target.
5. Rotation logic never directs a hostile cast at a foreign tagged creature. Healing and friendly
   targets behave as before.
6. The Package does not query any of the three live Loot Tag tables and does not copy party,
   membership or reward-range rules.
7. The Package does not create or mutate a Loot Tag.

## Tests

- Add focused tests for the pure policy covering no tag, own entitlement and foreign entitlement.
- Pin stale-current-target handling at the smallest testable seam.
- Pin that friendly/self rotation targets do not go through hostile ownership filtering.
- Run `./.github/check-core-tip.sh` against the clean LyraCore worktree at merge commit `7c96b65`.

## Non-goals

- Do not change corpse selection or loot action results. T2 owns those.
- Do not change Module Loot Tag, combat, quest or death code.
- Do not make grouped bots start independent quest work.
- Do not redesign combat targeting beyond the ownership check.

## Definition of done

The focused policy tests and the clean core-tip suite pass. The commit contains only the live-target
slice and its tests, and is pushed for integration before T2 starts.
