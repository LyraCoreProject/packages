# Issue #15: Playerbots consumes Loot Tag ownership

Source: [LyraCoreProject/packages#15](https://github.com/LyraCoreProject/packages/issues/15),
"playerbots: consume LyraCore Loot Tag ownership".

## State of the world

LyraCore PR #389 shipped issue #385 at merge commit
`7c96b65d48a856c8fd5fddeb35cb7477e8e26fc5`. The Module now owns Loot Tag creation,
tag-time party ceilings, leaver revocation, death entitlement, quest credit, corpse eligibility and
every creature-loot Gate. The Package must consume those answers. It must not read the three live
tag tables and rebuild their membership rules.

The Package branch starts at packages commit `d3144842f1980f9fb9dbb99b990e5d5d8946b4e3`.
Before any change, `./.github/check-core-tip.sh` against the merged LyraCore commit passed all
1,455 Module library tests. The existing `emit_bot_invite_intent` dead-code warning is baseline.

Current Playerbots behavior is concentrated in `playerbots/src/goals.rs`:

- `combat_target`, `work_quest`, `grind` and rotation target helpers do not consult Loot Tag
  ownership.
- `take_what_the_kill_left` tries coin and wanted item slots on every nearby creature corpse and
  drops every returned `Result`.
- Quest hunting reads the Module-owned `CharacterQuest.counts`. The Package does not award kill
  credit itself, which is correct.

## Core contract to consume

Package code compiles as `crate::pkg_playerbots` inside the LyraCore Module. It can call these
crate-visible functions:

```rust
crate::loot::death_entitlement(
    ctx,
    creature.guid,
    creature.x,
    creature.y,
    creature.map_id,
    creature.instance_id,
) -> Option<DeathEntitlement>

crate::loot::corpse_eligible_recipients(ctx, corpse_guid) -> Vec<u64>
crate::loot::corpse_eligible_for_access(&recipients, actor_guid) -> bool
```

For a live creature, no entitlement means untagged and available. An entitlement containing the
bot Character means the tag permits that Character. Any other entitlement is foreign. The target
is already inside the Package's 60-yard sight radius, which is inside the Module's 74-yard reward
range used by `death_entitlement`.

For a corpse, `corpse_eligible_recipients` is the canonical, sorted snapshot. Use
`corpse_eligible_for_access` to test it. Do not query `game_corpse_loot_eligible` directly.

The action functions remain authoritative:

```rust
crate::actor::loot_money(ctx, actor_guid, corpse_guid) -> Result<(), String>
crate::actor::take_loot(ctx, actor_guid, corpse_guid, slot) -> Result<(), String>
```

Their Loot Tag Refusal has this stable prefix:

```text
loot_tag_ineligible:
```

Use an exact `starts_with("loot_tag_ineligible:")` classifier. A match ends work on that corpse for
the current tick. The eligibility prefilter keeps the bot from choosing the same foreign corpse on
later ticks. Do not add Package-owned corpse state.

The Module awards quest credit to each entitled recipient during death dispatch, before it writes
corpse eligibility and clears the live tag. Killing-blow identity is not Package state. Playerbots
must keep reading `CharacterQuest.counts` and `quest_is_complete` on later brain ticks.

## Execution order

```text
T1 live target policy, serial tracer
  -> T2 corpse selection and Refusal handling
       -> T3 combined review and verification, serial integration
```

All tickets are serial because they touch the same decision loop in `playerbots/src/goals.rs`.
There is no safe parallel frontier after the tracer.

| Ticket | Model | Estimate | File ownership |
|---|---|---:|---|
| T1, live target policy | GPT-5.6 Sol, xhigh | ~170k | live combat, quest target and grind regions of `playerbots/src/goals.rs`, plus their focused tests |
| T2, corpse eligibility | GPT-5.6 Terra, high | ~150k | corpse-loot region and focused tests in `playerbots/src/goals.rs`; `playerbots/README.md` |
| T3, integration | GPT-5.6 Sol, xhigh | ~150k | the combined Playerbots diff; edit only to reconcile or fix an issue found during review |

## Shared rules

- Read the Package `AGENTS.md`, then LyraCore `CODING_STANDARDS.md` and `CONTEXT.md` before editing.
- Keep Loot Tag ownership in the Module. Do not copy tag rows, party snapshots, membership checks,
  range checks or death-credit rules into the Package.
- Use Character, Loot Tag, Gate and Refusal as defined in `CONTEXT.md`.
- `actor::attack` starts an Engagement. The first positive threat fixes the Loot Tag. Do not create
  or change a tag from Package code.
- Keep the immediate `on_damage_taken` self-defense hook. The regular brain tick must stop or skip
  a foreign tagged creature instead of adopting it as a goal.
- `open_creature_loot` is not part of the direct bot take path. Money and item actions apply their
  own Gates.
- Do not call `quest::on_creature_killed`, `combat::kill_creature` or a debug reducer from Package
  behavior.
- Apply the standards to changed lines without formatting or comment churn elsewhere.
- Do not touch a realm, live database or GitHub issue from an implementation ticket.

## Required verification

Every implementation ticket runs focused tests for its policy. T1 and T2 also run:

```bash
./.github/check-core-tip.sh /path/to/clean/LyraCore
```

T3 repeats that clean-tip check against the merged LyraCore commit, reviews the final diff, and
runs the existing standalone Loot Tag fixture when the local SpacetimeDB and wasm toolchain allow
it. A tool or legacy-lint failure must include a clean-base comparison.
