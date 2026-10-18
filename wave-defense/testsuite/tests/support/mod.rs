#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

#[allow(unused_imports)]
pub use lyracore_test_support::{poll_until, Standalone, POLL_TIMEOUT};

pub type Row = BTreeMap<String, String>;

pub fn number<T: std::str::FromStr>(row: &Row, column: &str) -> T {
    row[column]
        .parse()
        .ok()
        .expect("durable field must be a number")
}

pub struct Character {
    pub guid: String,
    pub account: String,
    pub class: String,
    pub actor: String,
}

pub struct Defense {
    pub node: Standalone,
    pub characters: Vec<Character>,
}

impl Defense {
    pub fn start(name: &str) -> Self {
        let mut node = Standalone::start(name);
        node.publish_module();
        node.assert_call("claim_operator", &[]);
        node.assert_call("install_guid_range", &["15000000"]);
        node.assert_call(
            "gw_certify_package_party_authority",
            &["\"wave-defense-test\"", "true"],
        );
        import_catalogs(&node);
        node.assert_call(
            "gw_certify_package_party_authority",
            &["\"wave-defense-test\"", "true"],
        );
        node.assert_call("wave_defense_fixture_stage", &[]);
        let mut rows = node.query_rows("SELECT guid, account_id, class, name FROM game_character");
        rows.retain(|row| row["name"].starts_with("Defense"));
        rows.sort_by_key(|row| row["guid"].parse::<u64>().unwrap());
        let characters = rows.into_iter().map(|row| {
            let guid = row["guid"].clone();
            let account = row["account_id"].clone();
            let nonce = guid.parse::<u64>().unwrap() + 101;
            node.assert_call("claim_account", &[&account, &guid, &nonce.to_string()]);
            let actor = format!(
                r#"{{"guid":{guid},"ownership":{{"some":{{"account_id":{account},"generation":1,"request_nonce":{nonce}}}}}}}"#,
            );
            Character { guid, account, class: row["class"].clone(), actor }
        }).collect::<Vec<_>>();
        assert_eq!(characters.len(), 10);
        Self { node, characters }
    }

    pub fn command(&self, index: usize, action: &str, payload: &str) {
        self.node.assert_call(
            "gw_client_command",
            &[
                &self.characters[index].actor,
                &serde_json::to_string(&format!("package.wave_defense.{action}")).unwrap(),
                &serde_json::to_string(payload).unwrap(),
            ],
        );
    }

    pub fn refused(&self, index: usize, action: &str, payload: &str, reason: &str) {
        self.refused_actor(&self.characters[index].actor, action, payload, reason);
    }

    pub fn refused_actor(&self, actor: &str, action: &str, payload: &str, reason: &str) {
        let before = self.node.query_rows("SELECT * FROM game_addon_message");
        let output = self.node.call(
            "gw_client_command",
            &[
                actor,
                &serde_json::to_string(&format!("package.wave_defense.{action}")).unwrap(),
                &serde_json::to_string(payload).unwrap(),
            ],
        );
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!output.status.success() && text.contains(reason), "{text}");
        assert_eq!(
            self.node.query_rows("SELECT * FROM game_addon_message"),
            before
        );
    }

    pub fn select(&self, index: usize) {
        self.command(
            index,
            "select",
            &format!("1|{}", self.characters[index].class),
        );
    }

    pub fn queue(&self, index: usize) {
        self.command(index, "queue", "2");
    }

    pub fn start_team(&self, first: usize) -> Row {
        for index in first..first + 5 {
            self.select(index);
            self.queue(index);
        }
        self.advance(0);
        self.run(first)
    }

    pub fn advance(&self, seconds: u32) {
        self.node.assert_call(
            "gw_certify_package_party_authority",
            &["\"wave-defense-test\"", "true"],
        );
        self.node
            .assert_call("wave_defense_fixture_advance", &[&seconds.to_string()]);
    }

    pub fn run(&self, index: usize) -> Row {
        let member = &self.node.query_rows(&format!(
            "SELECT match_id FROM pkg_wave_defense_member WHERE character_guid = {}",
            self.characters[index].guid,
        ))[0];
        self.node.query_rows(&format!(
            "SELECT * FROM pkg_wave_defense_match WHERE id = {}",
            member["match_id"]
        ))[0]
            .clone()
    }

    pub fn member(&self, index: usize) -> Row {
        self.node.query_rows(&format!(
            "SELECT * FROM pkg_wave_defense_member WHERE character_guid = {}",
            self.characters[index].guid,
        ))[0]
            .clone()
    }

    pub fn clear(&self, match_id: &str) {
        self.node
            .assert_call("wave_defense_fixture_clear", &[match_id]);
    }

    pub fn offer(&self, index: usize) -> Vec<String> {
        self.read(index, "offer")
    }

    pub fn phase(&self, index: usize) -> String {
        self.read(index, "status")[1].clone()
    }

    pub fn read(&self, index: usize, action: &str) -> Vec<String> {
        self.command(index, action, "");
        let messages = self.node.query_rows(&format!(
            "SELECT id, payload FROM game_addon_message WHERE cmd = 'package.wave_defense.{action}.result'",
        ));
        messages
            .iter()
            .max_by_key(|row| number::<u64>(row, "id"))
            .expect("offer reply missing")["payload"]
            .split('|')
            .map(str::to_owned)
            .collect()
    }

    pub fn choose(&self, index: usize, request: u64) -> Vec<String> {
        let offer = self.offer(index);
        assert!(offer.len() >= 9, "missing choices: {offer:?}");
        self.command(
            index,
            "choose",
            &format!("{request}|{}|{}|{}", offer[1], offer[2], offer[5]),
        );
        offer
    }
}

fn importer() -> &'static PathBuf {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(|| {
        if let Some(binary) = std::env::var_os("LYRACORE_TEST_IMPORTER") {
            return PathBuf::from(binary);
        }
        let output = Command::new("cargo")
            .current_dir(lyracore_test_support::core_root())
            .args([
                "build",
                "--locked",
                "-p",
                "lyracore-importer",
                "--message-format=json",
            ])
            .output()
            .expect("could not build the importer");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|message| message["target"]["name"] == "lyracore-importer")
            .find_map(|message| message["executable"].as_str().map(PathBuf::from))
            .expect("Cargo did not return the importer binary")
    })
}

struct PrivateConfig(PathBuf);

impl Drop for PrivateConfig {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn import_catalogs(node: &Standalone) {
    let client = std::env::var("LYRACORE_TEST_CLIENT_DATA")
        .expect("LYRACORE_TEST_CLIENT_DATA must name the Operator's WoW 1.12.1 Data directory");
    for mode in ["--spells", "--talents"] {
        let output = private_importer(node, &["--dbc", &client, mode]);
        assert!(
            output.status.success(),
            "catalogue import {mode}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

pub fn private_importer(node: &Standalone, arguments: &[&str]) -> Output {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let private = PrivateConfig(std::env::temp_dir().join(format!(
        "wave-defense-import-{}-{nonce}.toml",
        std::process::id()
    )));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&private.0)
        .expect("could not create private importer config");
    writeln!(file, "spacetimedb_token = {:?}", node.owner_token()).unwrap();
    drop(file);
    Command::new(importer())
        .env("LYRACORE_SPACETIME_CONFIG_PATH", &private.0)
        .args(arguments)
        .args([
            "--apply",
            "--db",
            node.shard_name(),
            "--server",
            node.server(),
        ])
        .output()
        .expect("could not run the isolated import")
}
