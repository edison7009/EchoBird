use std::{collections::BTreeSet, env, fs, path::PathBuf};
fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("..");
    let index = root
        .join("docs/api/tools/install/index.json")
        .canonicalize()
        .unwrap();
    println!("cargo:rerun-if-changed={}", index.display());
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&index).unwrap()).unwrap();
    let mut refs = String::new();
    let mut ids = String::new();
    let mut seen = BTreeSet::new();
    for id in value["ids"].as_array().expect("install ids") {
        let id = id.as_str().expect("tool id");
        assert!(
            !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            "invalid tool id"
        );
        assert!(seen.insert(id), "duplicate tool id");
        let path = root
            .join(format!("docs/api/tools/install/{id}.json"))
            .canonicalize()
            .unwrap();
        let reference: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(reference["id"], id, "install reference id mismatch");
        println!("cargo:rerun-if-changed={}", path.display());
        refs.push_str(&format!("({id:?}, include_str!({path:?})),\n"));
        ids.push_str(&format!("{id:?},"));
    }
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    fs::write(out.join("install_bundle.rs"), format!("static BUNDLED: BundledAssets = BundledAssets {{ install_index_json: include_str!({index:?}), install_refs: &[{refs}] }};\n")).unwrap();
    fs::write(
        out.join("install_ids.rs"),
        format!("pub const INSTALLABLE_TOOL_IDS: &[&str] = &[{ids}];\n"),
    )
    .unwrap();
    tauri_build::build()
}
