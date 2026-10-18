use super::*;
use std::collections::BTreeSet;

#[test]
fn launch_specs_cover_each_class_and_role() {
    assert_eq!(SPECS.len(), 9);
    assert_eq!(
        SPECS.iter().filter(|spec| spec.role == Role::Tank).count(),
        2
    );
    assert_eq!(
        SPECS
            .iter()
            .filter(|spec| spec.role == Role::Healer)
            .count(),
        2
    );
    assert_eq!(
        SPECS
            .iter()
            .filter(|spec| spec.role == Role::Damage)
            .count(),
        5
    );
    assert_eq!(
        SPECS.iter().map(|spec| spec.class).collect::<BTreeSet<_>>(),
        BTreeSet::from([1, 2, 3, 4, 5, 7, 8, 9, 11])
    );
    for spec in &SPECS {
        assert_eq!(spec_for_class(spec.class), Some(spec));
        assert!(!spec.essential_spells.is_empty());
        assert!(spec.draft_spells.len() >= 15, "{}", spec.name);
        let spells = spec
            .essential_spells
            .iter()
            .chain(spec.draft_spells)
            .copied();
        assert_eq!(
            spells.collect::<BTreeSet<_>>().len(),
            spec.essential_spells.len() + spec.draft_spells.len(),
            "{} repeats a spell",
            spec.name
        );
    }
    assert_eq!(spec_for_class(0), None);
    assert_eq!(spec_for_class(6), None);
    assert_eq!(spec_for_class(u8::MAX), None);
}

#[test]
fn reward_milestones_include_level_one_and_every_fifth_level() {
    assert_eq!(
        REWARD_LEVELS,
        [1, 5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60]
    );
}

#[test]
fn bosses_arrive_at_waves_five_ten_and_fifteen() {
    assert_eq!(WAVES.len(), 15);
    assert_eq!(
        WAVES
            .iter()
            .filter(|wave| wave.boss)
            .map(|wave| wave.number)
            .collect::<Vec<_>>(),
        vec![5, 10, 15]
    );
    for number in 1..=15 {
        let entry = wave(number).unwrap();
        assert_eq!(entry.number, number);
        assert!((1..=3).contains(&entry.fronts));
        assert!(entry.spawn_count >= u16::from(entry.fronts));
        assert!(entry.max_health_permille >= 1_000);
        assert!(entry.damage_permille >= 1_000);
    }
    assert_eq!(wave(0), None);
    assert_eq!(wave(16), None);
    assert_eq!(wave(u8::MAX), None);
}

#[test]
fn the_team_reaches_sixty_before_wave_thirteen() {
    assert_eq!(
        WAVES.iter().map(|wave| wave.level).collect::<Vec<_>>(),
        vec![1, 5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 60, 60]
    );
    assert_eq!(wave(1).unwrap().level, 1);
    assert_eq!(wave(13).unwrap().level, 60);
}

#[test]
fn pacing_budget_targets_thirty_minutes() {
    let combat_seconds = WAVES
        .iter()
        .map(|wave| u32::from(wave.target_combat_seconds))
        .sum::<u32>();
    let preparation_seconds = 15 * u32::from(PREPARATION_SECONDS);
    assert_eq!(combat_seconds + preparation_seconds, 30 * 60);
}
