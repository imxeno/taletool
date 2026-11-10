use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
};

fn files(path: &Path, output: &mut BTreeSet<PathBuf>) {
    if path.is_dir() {
        for child in fs::read_dir(path).unwrap() {
            files(&child.unwrap().path(), output);
        }
    } else if path.extension().is_some_and(|ext| ext == "rs") {
        output.insert(path.to_owned());
    }
}

fn dependencies(lock: &toml::Value, roots: &[&str]) -> Vec<serde_json::Value> {
    let packages = lock["package"].as_array().unwrap();
    let mut wanted: Vec<String> = roots.iter().map(|name| (*name).to_owned()).collect();
    let mut included = BTreeMap::new();
    while let Some(dependency) = wanted.pop() {
        let parts: Vec<_> = dependency.split_whitespace().collect();
        for package in packages.iter().filter(|package| {
            package["name"].as_str() == Some(parts[0])
                && (parts.len() == 1 || package["version"].as_str() == Some(parts[1]))
        }) {
            let name = package["name"].as_str().unwrap();
            if name.starts_with("taletool") {
                continue;
            }
            let key = format!("{}@{}", name, package["version"].as_str().unwrap());
            if included.contains_key(&key) {
                continue;
            }
            if let Some(deps) = package.get("dependencies").and_then(|v| v.as_array()) {
                wanted.extend(deps.iter().map(|dep| dep.as_str().unwrap().to_owned()));
            }
            included.insert(key, serde_json::to_value(package).unwrap());
        }
    }
    included.into_values().collect()
}

fn main() {
    let crate_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let root = crate_dir.join("../..").canonicalize().unwrap();
    let lock: toml::Value =
        toml::from_str(&fs::read_to_string(root.join("Cargo.lock")).unwrap()).unwrap();
    println!(
        "cargo:rerun-if-changed={}",
        root.join("Cargo.lock").display()
    );
    let mut components = BTreeMap::new();
    let mut recipes: Vec<(String, Vec<&str>, Vec<&str>)> = vec![(
        "extract.text".into(),
        vec![
            "crates/taletool-cli/src/pipeline/mod.rs",
            "crates/taletool-cli/src/pipeline/extract.rs",
            "crates/taletool-cli/src/output.rs",
            "crates/taletool-archive/src/text.rs",
            "crates/taletool-core/src",
        ],
        vec!["anyhow", "serde", "serde_json", "sha2", "hex", "thiserror"],
    )];
    for parser in [
        "item",
        "bcard",
        "card",
        "skill",
        "monster",
        "quest",
        "quest-prize",
        "quest-npc",
        "map-ids",
        "language",
        "constants",
    ] {
        let mut inputs = vec![
            "crates/taletool-cli/src/pipeline/mod.rs",
            "crates/taletool-cli/src/pipeline/parse.rs",
            "crates/taletool-cli/src/structured_text_file.rs",
            "crates/taletool-cli/src/text_payload.rs",
            "crates/taletool-text/src/lib.rs",
            "crates/taletool-core/src",
        ];
        if !matches!(parser, "language" | "constants") {
            inputs.push("crates/taletool-text/src/gtd");
        }
        recipes.push((
            format!("parse.{parser}"),
            inputs,
            vec!["anyhow", "serde", "serde_json", "encoding_rs", "thiserror"],
        ));
    }
    for (id, inputs, deps) in recipes {
        let mut sources = BTreeSet::new();
        for path in inputs {
            println!("cargo:rerun-if-changed={}", root.join(path).display());
            files(&root.join(path), &mut sources);
        }
        let mut hash = Sha256::new();
        hash.update(b"taletool-pipeline-v1\0");
        hash.update(id.as_bytes());
        let mut names = Vec::new();
        for source in sources {
            let name = source
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = fs::read(&source).unwrap();
            hash.update((name.len() as u64).to_le_bytes());
            hash.update(name.as_bytes());
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(bytes);
            names.push(name);
        }
        let workspace: toml::Value =
            toml::from_str(&fs::read_to_string(root.join("Cargo.toml")).unwrap()).unwrap();
        println!(
            "cargo:rerun-if-changed={}",
            root.join("Cargo.toml").display()
        );
        let source_text = names
            .iter()
            .map(|name| fs::read_to_string(root.join(name)).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
            .split_whitespace()
            .collect::<String>();
        let declarations = workspace["workspace"]["dependencies"].as_table().unwrap();
        let mut roots: BTreeSet<String> = deps.iter().map(|name| (*name).to_owned()).collect();
        for name in declarations.keys() {
            if name.starts_with("taletool") {
                continue;
            }
            let rust_name = name.replace('-', "_");
            if source_text.contains(&format!("{rust_name}::"))
                || source_text.contains(&format!("use{rust_name}as"))
                || source_text.contains(&format!("externcrate{rust_name}"))
            {
                roots.insert(name.clone());
            }
        }
        let owners: BTreeSet<_> = names
            .iter()
            .map(|name| name.split('/').nth(1).unwrap())
            .collect();
        let mut settings = BTreeMap::new();
        for owner in &owners {
            let path = root.join("crates").join(owner).join("Cargo.toml");
            println!("cargo:rerun-if-changed={}", path.display());
            let manifest: toml::Value = toml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
            let dependencies: BTreeMap<_, _> = manifest["dependencies"]
                .as_table()
                .unwrap()
                .iter()
                .filter(|(name, _)| roots.contains(*name) || owners.contains(name.as_str()))
                .collect();
            settings.insert(
                (*owner).to_owned(),
                serde_json::to_value(dependencies).unwrap(),
            );
        }
        let declarations: BTreeMap<_, _> = roots
            .iter()
            .map(|name| (name, declarations.get(name)))
            .collect();
        let settings = json!({ "workspace": declarations, "crates": settings });
        let root_names: Vec<_> = roots.iter().map(String::as_str).collect();
        let deps = dependencies(&lock, &root_names);
        hash.update(serde_json::to_vec(&settings).unwrap());
        hash.update(serde_json::to_vec(&deps).unwrap());
        components.insert(id, json!({"schema": 1, "fingerprint": format!("{:x}",hash.finalize()), "sources": names, "dependencies": deps,"settings":settings}));
    }
    let path = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("pipeline-components.json");
    fs::write(
        path,
        serde_json::to_vec_pretty(&json!({"version":1,"components":components})).unwrap(),
    )
    .unwrap();
}
