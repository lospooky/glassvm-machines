use std::{
    fs,
    path::{Path, PathBuf},
};

const FORBIDDEN_SURFACES: &[&str] = &[
    "ObservationPlan",
    "RunConfig",
    "TraceChunk",
    "ReplayManifest",
    "ReplayPackage",
    "ReplayReceipt",
    "ObservableAdapter",
    "native_event_adapter",
    "observable_adapter",
    "record_function",
    "record_run",
    "body_provider",
    "CapabilityBlob",
    "glassvm_episode",
    "create_execution_with_prepared_observation",
    "glassvm.python_bundle.v1",
    "fn create_execution(\n",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("conformance crate must be nested beneath machines")
        .to_path_buf()
}

fn publication_source_roots(root: &Path) -> Vec<PathBuf> {
    let mut roots = vec![
        root.join("chip8/python/src"),
        root.join("hexwell/python/src"),
        root.join("wyrd16/python/src"),
        root.join("pico8/python/src"),
        root.join("tic80/python/src"),
    ];
    for machine in ["chip8", "hexwell", "wyrd16", "pico8", "tic80"] {
        for role in ["core", "verifier", "plugin"] {
            roots.push(root.join(machine).join(role).join("src"));
        }
    }
    roots
}

fn source_files(root: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            source_files(&path, files);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "rs" || extension == "toml" || extension == "py")
        {
            files.push(path);
        }
    }
}

#[test]
fn publication_sources_exclude_removed_contract_surfaces() {
    let root = workspace_root();
    let mut files = Vec::new();
    for source_root in publication_source_roots(&root) {
        source_files(&source_root, &mut files);
    }

    let mut violations = Vec::new();
    for path in files {
        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        for forbidden in FORBIDDEN_SURFACES {
            if contents.contains(forbidden) {
                violations.push(format!("{} contains {forbidden:?}", path.display()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "publication source contains removed contract surfaces:\n{}",
        violations.join("\n")
    );
}
