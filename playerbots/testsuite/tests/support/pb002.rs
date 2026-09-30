use super::evidence::{digest_files, git};

const PB002_CORE: &str = "e6a755db0a150bbf73ad97b972fe829f20f6816c";
const PB002_CORE_TREE: &str = "0769d6b7cd96d7e23a7ad16528399544ece0e2ec";
const PB002_COLLECTION: &str = "155c9e401afb06d5731acedf8fc35a81dbe4aaa6";
const PB002_COLLECTION_TREE: &str = "e2558a9cf421a74f79497ef361dbbfceb911a0d1";
const PB002_PACKAGE_IDENTITY: &str =
    "33fcb8a217aad84f46f9ad65efdad23be24bad8e29828777de7c7e6469417161";

pub struct PrecedingPb002 {
    pub wasm: Vec<u8>,
    pub manifest: serde_json::Value,
}

pub fn preceding_pb002() -> PrecedingPb002 {
    let wasm_path = std::env::var_os("PLAYERBOTS_COMPANION_PRECEDING_WASM")
        .expect("PLAYERBOTS_COMPANION_PRECEDING_WASM must name the merged PB-002 Wasm");
    let manifest_path = std::env::var_os("PLAYERBOTS_COMPANION_PRECEDING_MANIFEST")
        .expect("PLAYERBOTS_COMPANION_PRECEDING_MANIFEST must describe that Wasm build");
    let core_path = std::env::var_os("PLAYERBOTS_COMPANION_PRECEDING_CORE")
        .expect("PLAYERBOTS_COMPANION_PRECEDING_CORE must name the clean PB-002 checkout");
    let collection_path = std::env::var_os("PLAYERBOTS_COMPANION_PRECEDING_COLLECTION")
        .expect("PLAYERBOTS_COMPANION_PRECEDING_COLLECTION must name the clean PB-002 checkout");
    let core_path = std::path::Path::new(&core_path);
    let collection_path = std::path::Path::new(&collection_path);
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(manifest_path).unwrap()).unwrap();
    let wasm = std::fs::read(&wasm_path).unwrap();

    assert_eq!(manifest["core"], PB002_CORE);
    assert_eq!(manifest["collection"], PB002_COLLECTION);
    assert_eq!(manifest["core_tree"], PB002_CORE_TREE);
    assert_eq!(manifest["collection_tree"], PB002_COLLECTION_TREE);
    assert_eq!(manifest["core_dirty"], false);
    assert_eq!(manifest["collection_dirty"], false);
    assert_eq!(manifest["rust"], "1.93.0");
    assert_eq!(manifest["spacetimedb"], "2.7.1");
    assert_eq!(manifest["target"], "wasm32-unknown-unknown");
    assert_eq!(manifest["profile"], "release");
    assert_eq!(manifest["features"], serde_json::json!(["debug_reducers"]));
    assert_eq!(
        manifest["installed_packages"],
        serde_json::json!(["dungeons", "example", "fire_nova", "playerbots"])
    );
    assert_eq!(manifest["package_content_identity"], PB002_PACKAGE_IDENTITY);
    assert_eq!(manifest["wasm_bytes"].as_u64(), Some(wasm.len() as u64));

    assert_eq!(git(core_path, &["rev-parse", "HEAD"]), PB002_CORE);
    assert_eq!(
        git(core_path, &["rev-parse", "HEAD^{tree}"]),
        PB002_CORE_TREE
    );
    assert!(git(core_path, &["status", "--porcelain"]).is_empty());
    assert_eq!(
        git(collection_path, &["rev-parse", "HEAD"]),
        PB002_COLLECTION
    );
    assert_eq!(
        git(collection_path, &["rev-parse", "HEAD^{tree}"]),
        PB002_COLLECTION_TREE
    );
    if let Some(playerbots_tree) = manifest["playerbots_tree"].as_str() {
        assert_eq!(
            git(collection_path, &["rev-parse", "HEAD:playerbots"]),
            playerbots_tree
        );
    }
    assert!(git(collection_path, &["status", "--porcelain"]).is_empty());
    let mut package_digest = blake3::Hasher::new();
    digest_files(&collection_path.join("playerbots"), &mut package_digest);
    assert_eq!(
        package_digest.finalize().to_hex().as_str(),
        PB002_PACKAGE_IDENTITY
    );

    let sha256 = std::process::Command::new("sha256sum")
        .arg(&wasm_path)
        .output()
        .unwrap();
    assert!(sha256.status.success());
    let sha256 = String::from_utf8(sha256.stdout).unwrap();
    assert_eq!(
        sha256.split_whitespace().next().unwrap(),
        manifest["wasm_sha256"].as_str().unwrap()
    );
    if let Some(expected) = manifest["wasm_blake3"].as_str() {
        assert_eq!(blake3::hash(&wasm).to_hex().as_str(), expected);
    }

    PrecedingPb002 { wasm, manifest }
}
