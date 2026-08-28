//! The `dungeons` Package: encounter choreography for every scripted dungeon, one submodule per
//! dungeon. The registry scan resolves every file through `crate::pkg_dungeons`, so each
//! submodule's markers must stay visible through the glob re-exports below. Adding a dungeon is
//! one new submodule file plus its `mod`/`pub use` pair. See README.md for the Package boundary.

mod blackfathom_deeps;
mod blackrock_depths;
mod dire_maul;
mod eventai_instance_test;
mod razorfen_kraul;
mod shadowfang_keep;
mod sunken_temple;
mod wailing_caverns;
mod zulgurub;

pub(crate) use blackfathom_deeps::*;
pub(crate) use blackrock_depths::*;
pub(crate) use dire_maul::*;
pub(crate) use razorfen_kraul::*;
pub(crate) use shadowfang_keep::*;
pub(crate) use sunken_temple::*;
pub(crate) use wailing_caverns::*;
pub(crate) use zulgurub::*;

// This assertion lives in the Package, not in core, because only an installed `dungeons` can
// promise full coverage: a bare core ships the `EncounterBinding` enum with no authorities at all.
#[cfg(test)]
mod tests {
    #[test]
    fn every_encounter_binding_has_exactly_one_installed_authority() {
        let mut from_enum: Vec<String> = crate::encounter::EncounterBinding::ALL
            .iter()
            .map(|binding| format!("{binding:?}"))
            .collect();
        from_enum.sort();
        assert_eq!(crate::GAME_ENCOUNTER_PACKAGE_BINDING_NAMES, from_enum);
        assert_eq!(
            crate::GAME_ENCOUNTER_PACKAGE_BINDING_NAMES,
            [
                "BlackfathomDeepsKelris",
                "BlackrockDepthsTombOfSeven",
                "DireMaulAlzzin",
                "RazorfenKraulWardKeepers",
                "ShadowfangKeepFenrus",
                "ShadowfangKeepNandos",
                "ShadowfangKeepRethilgore",
                "SunkenTempleAvatar",
                "WailingCavernsAnacondra",
                "WailingCavernsCobrahn",
                "WailingCavernsMutanus",
                "WailingCavernsPythas",
                "WailingCavernsSerpentis",
                "ZulGurubOhgan",
            ]
        );
    }
}
