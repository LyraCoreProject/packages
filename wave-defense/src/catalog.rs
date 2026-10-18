#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    Tank,
    Healer,
    Damage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Spec {
    pub(crate) class: u8,
    pub(crate) name: &'static str,
    pub(crate) role: Role,
    pub(crate) talent_tree: u32,
    pub(crate) essential_spells: &'static [u32],
    pub(crate) draft_spells: &'static [u32],
}

pub(crate) const SPECS: [Spec; 9] = [
    Spec {
        class: 1,
        name: "Protection Warrior",
        role: Role::Tank,
        talent_tree: 163,
        essential_spells: &[2457, 71, 355, 7386, 78],
        draft_spells: &[
            100, 772, 6673, 6343, 1715, 676, 694, 2565, 6572, 72, 871, 5246, 1160, 2687, 20230,
            20252, 2458, 6552,
        ],
    },
    Spec {
        class: 11,
        name: "Feral Bear Druid",
        role: Role::Tank,
        talent_tree: 281,
        essential_spells: &[5487, 6795, 6807, 779],
        draft_spells: &[
            1126, 467, 774, 5185, 8936, 20484, 5211, 5209, 5229, 770, 8921, 5176, 2782, 8946, 2893,
            22812, 29166,
        ],
    },
    Spec {
        class: 5,
        name: "Holy Priest",
        role: Role::Healer,
        talent_tree: 202,
        essential_spells: &[2050, 2061, 17, 139],
        draft_spells: &[
            586, 588, 1243, 2006, 527, 528, 552, 596, 2054, 2060, 21562, 8092, 589, 8122, 605,
            9484, 976,
        ],
    },
    Spec {
        class: 7,
        name: "Restoration Shaman",
        role: Role::Healer,
        talent_tree: 262,
        essential_spells: &[331, 8004],
        draft_spells: &[
            403, 8050, 8042, 8056, 324, 8017, 8075, 8071, 5394, 5675, 5730, 8143, 8177, 8512, 2484,
            1064, 526, 2870, 2008, 370,
        ],
    },
    Spec {
        class: 2,
        name: "Retribution Paladin",
        role: Role::Damage,
        talent_tree: 381,
        essential_spells: &[21084, 20271],
        draft_spells: &[
            635, 19750, 19740, 465, 7294, 19742, 20164, 20165, 498, 642, 1022, 1044, 633, 853, 879,
            1152, 4987, 7328, 6940, 1038, 2812,
        ],
    },
    Spec {
        class: 4,
        name: "Combat Rogue",
        role: Role::Damage,
        talent_tree: 181,
        essential_spells: &[1752, 2098],
        draft_spells: &[
            1784, 53, 703, 1776, 408, 1766, 5277, 2983, 1856, 1833, 1966, 8647, 1943, 5171, 1725,
            2094,
        ],
    },
    Spec {
        class: 3,
        name: "Beast Mastery Hunter",
        role: Role::Damage,
        talent_tree: 361,
        essential_spells: &[75, 883, 1515, 2641, 6991, 982],
        draft_spells: &[
            136, 1978, 3044, 1130, 5116, 2973, 1495, 2974, 5118, 13165, 13163, 5384, 1499, 13809,
            13813, 13795, 3043, 1510, 2643,
        ],
    },
    Spec {
        class: 8,
        name: "Frost Mage",
        role: Role::Damage,
        talent_tree: 61,
        essential_spells: &[116, 168],
        draft_spells: &[
            122, 10, 120, 7302, 6143, 1459, 1463, 118, 2139, 1953, 475, 1449, 5143, 604, 5504, 587,
            543, 2136,
        ],
    },
    Spec {
        class: 9,
        name: "Affliction Warlock",
        role: Role::Damage,
        talent_tree: 302,
        essential_spells: &[686, 688],
        draft_spells: &[
            172, 348, 980, 702, 1490, 6201, 693, 1454, 689, 755, 5782, 5484, 5138, 710, 6229, 1098,
            1120, 5676, 1714, 6789, 6353,
        ],
    },
];

pub(crate) fn spec_for_class(class: u8) -> Option<&'static Spec> {
    SPECS.iter().find(|spec| spec.class == class)
}

pub(crate) const REWARD_LEVELS: [u8; 13] = [1, 5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60];
pub(crate) const PREPARATION_SECONDS: u16 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Wave {
    pub(crate) number: u8,
    pub(crate) name: &'static str,
    pub(crate) level: u8,
    pub(crate) boss: bool,
    pub(crate) fronts: u8,
    pub(crate) spawn_count: u16,
    pub(crate) max_health_permille: u16,
    pub(crate) damage_permille: u16,
    pub(crate) target_combat_seconds: u16,
}

pub(crate) const WAVES: [Wave; 15] = [
    Wave {
        number: 1,
        name: "Gate scouts",
        level: 1,
        boss: false,
        fronts: 1,
        spawn_count: 8,
        max_health_permille: 1000,
        damage_permille: 1000,
        target_combat_seconds: 90,
    },
    Wave {
        number: 2,
        name: "Courtyard raiders",
        level: 5,
        boss: false,
        fronts: 1,
        spawn_count: 10,
        max_health_permille: 1075,
        damage_permille: 1035,
        target_combat_seconds: 90,
    },
    Wave {
        number: 3,
        name: "Shield line",
        level: 10,
        boss: false,
        fronts: 1,
        spawn_count: 12,
        max_health_permille: 1150,
        damage_permille: 1070,
        target_combat_seconds: 90,
    },
    Wave {
        number: 4,
        name: "Broken barricade",
        level: 15,
        boss: false,
        fronts: 2,
        spawn_count: 14,
        max_health_permille: 1225,
        damage_permille: 1105,
        target_combat_seconds: 90,
    },
    Wave {
        number: 5,
        name: "The gate captain",
        level: 20,
        boss: true,
        fronts: 1,
        spawn_count: 9,
        max_health_permille: 5000,
        damage_permille: 1300,
        target_combat_seconds: 140,
    },
    Wave {
        number: 6,
        name: "East assault",
        level: 25,
        boss: false,
        fronts: 1,
        spawn_count: 16,
        max_health_permille: 1375,
        damage_permille: 1175,
        target_combat_seconds: 90,
    },
    Wave {
        number: 7,
        name: "West assault",
        level: 30,
        boss: false,
        fronts: 1,
        spawn_count: 18,
        max_health_permille: 1450,
        damage_permille: 1210,
        target_combat_seconds: 90,
    },
    Wave {
        number: 8,
        name: "Twin fronts",
        level: 35,
        boss: false,
        fronts: 2,
        spawn_count: 20,
        max_health_permille: 1525,
        damage_permille: 1245,
        target_combat_seconds: 90,
    },
    Wave {
        number: 9,
        name: "Courtyard siege",
        level: 40,
        boss: false,
        fronts: 2,
        spawn_count: 22,
        max_health_permille: 1600,
        damage_permille: 1280,
        target_combat_seconds: 90,
    },
    Wave {
        number: 10,
        name: "The siege commander",
        level: 45,
        boss: true,
        fronts: 2,
        spawn_count: 13,
        max_health_permille: 6500,
        damage_permille: 1600,
        target_combat_seconds: 140,
    },
    Wave {
        number: 11,
        name: "Outer ring",
        level: 50,
        boss: false,
        fronts: 2,
        spawn_count: 24,
        max_health_permille: 1750,
        damage_permille: 1350,
        target_combat_seconds: 90,
    },
    Wave {
        number: 12,
        name: "Closing ranks",
        level: 55,
        boss: false,
        fronts: 2,
        spawn_count: 26,
        max_health_permille: 1825,
        damage_permille: 1385,
        target_combat_seconds: 90,
    },
    Wave {
        number: 13,
        name: "The last muster",
        level: 60,
        boss: false,
        fronts: 3,
        spawn_count: 28,
        max_health_permille: 1900,
        damage_permille: 1420,
        target_combat_seconds: 90,
    },
    Wave {
        number: 14,
        name: "All fronts",
        level: 60,
        boss: false,
        fronts: 3,
        spawn_count: 30,
        max_health_permille: 1975,
        damage_permille: 1455,
        target_combat_seconds: 90,
    },
    Wave {
        number: 15,
        name: "The final commander",
        level: 60,
        boss: true,
        fronts: 3,
        spawn_count: 17,
        max_health_permille: 8000,
        damage_permille: 1900,
        target_combat_seconds: 140,
    },
];

pub(crate) fn wave(number: u8) -> Option<&'static Wave> {
    WAVES.get(usize::from(number.checked_sub(1)?))
}

#[cfg(test)]
mod tests;
