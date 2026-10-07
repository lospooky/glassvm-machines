use std::path::Path;

use glassvm_machine_conformance::PUBLICATION_MACHINES;

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("conformance crate must be nested beneath the workspace root")
}

#[test]
fn publication_layout_names_the_three_release_bundles() {
    let root = workspace_root();
    let mut discovered = std::fs::read_dir(root)
        .expect("machine workspace root")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| PUBLICATION_MACHINES.contains(&name.as_str()))
        .collect::<Vec<_>>();
    discovered.sort();

    assert_eq!(discovered, PUBLICATION_MACHINES);
}

#[test]
fn each_publication_bundle_has_the_expected_release_crates_and_fixture() {
    let root = workspace_root();

    for machine in PUBLICATION_MACHINES {
        let bundle = root.join(machine);
        for role in ["core", "verifier", "bundle", "python"] {
            assert!(
                bundle.join(role).join("Cargo.toml").is_file(),
                "{machine}: missing {role} crate manifest"
            );
        }
        assert!(
            bundle.join("fixtures/smoke.rom").is_file(),
            "{machine}: missing smoke fixture"
        );
        assert!(
            bundle.join("pyproject.toml").is_file(),
            "{machine}: missing bundle-root Python project"
        );
        assert!(
            !bundle.join("python/pyproject.toml").exists(),
            "{machine}: Python project must be rooted at the bundle directory"
        );
    }
}
