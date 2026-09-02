# T2: select only eligible corpses and handle ownership Refusals

Parent: issue #15. **Depends on integrated T1. Runs alone and blocks T3.**
Model: GPT-5.6 Terra, high. Estimated size: ~150k tokens.

## Problem

`take_what_the_kill_left` scans every nearby creature corpse. It asks for coin, reads wanted item
slots and asks to take them without checking corpse eligibility. Both action functions enforce the
Module Gate, so foreign loot stays safe, but the bot repeats the same refused work every brain tick
and drops the reason.

The Package also needs to distinguish the stable Loot Tag Refusal from an unrelated action failure.
A Loot Tag Refusal means the corpse is not this Character's goal. It is not a transport failure.

## Delivery

Before reading money or loot rows for a corpse, call
`crate::loot::corpse_eligible_recipients(ctx, corpse.guid)` and then
`crate::loot::corpse_eligible_for_access(&recipients, me.guid)`. Skip the corpse unless it permits
the bot Character. Do not read `game_corpse_loot_eligible` directly.

Keep the action functions authoritative. Continue to call `actor::loot_money` and
`actor::take_loot`; do not replace their Gates with the prefilter. Inspect each returned `Result`
instead of discarding it.

Add one exact classifier for reasons that start with `loot_tag_ineligible:`. If either money or an
item take returns that Refusal, stop all work on that corpse for the current tick and continue with
the next candidate. Do not retry another slot on it. A nonmatching result must keep the Package's
existing gameplay behavior and must not be mislabeled as Loot Tag ownership.

There is no persisted corpse goal today. Do not add one. The canonical eligibility prefilter makes
later ticks skip foreign corpses, while the Refusal branch closes the small gap between selection
and the authoritative action Gate.

Update `playerbots/README.md` so it says the bot takes coin and wanted quest items only from an
eligible creature corpse. State that Module death entitlement supplies quest credit, regardless of
which unit lands the lethal hit. Keep the documentation brief and use the terms from `CONTEXT.md`.

## Acceptance criteria

1. A corpse is considered only when the Module's canonical recipient set contains the bot
   Character.
2. The Package does not read money or `game_corpse_loot` rows for a foreign corpse and calls no
   loot action for it.
3. An eligible corpse still yields coin and wanted quest items through the existing action
   functions.
4. A `loot_tag_ineligible:` result from money stops item attempts for that corpse.
5. The same Refusal from an item stops later slots for that corpse.
6. A string that merely contains the marker later, or begins with a similar spelling, is not
   classified as the Loot Tag Refusal.
7. The bot makes its normal quest, grind or follow decision after it skips a foreign corpse. No
   retry state or new table is added.
8. `open_creature_loot` is not added to the direct bot path.

## Tests

- Add focused tests for eligible and foreign corpse decisions.
- Test the exact Refusal prefix and at least two false positives.
- Drive the corpse action loop through a small test seam or adapter. Prove that a money Refusal
  suppresses item calls and an item Refusal suppresses later slots.
- Run `./.github/check-core-tip.sh` against the clean LyraCore worktree at merge commit `7c96b65`.

## Non-goals

- Do not change live target policy from T1.
- Do not recreate quest-only or group-loot rules. `take_loot` owns them.
- Do not add corpse ownership to Playerbots durable tables.
- Do not change the Module, Gateway or protocol.

## Definition of done

Focused corpse tests and the clean core-tip suite pass. The README matches the behavior. The commit
contains the corpse slice, its tests and that small documentation update, and is pushed before T3
starts.
