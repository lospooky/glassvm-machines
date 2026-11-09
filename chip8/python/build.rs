use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets CARGO_MANIFEST_DIR"),
    );
    let pyproject = manifest_dir
        .ancestors()
        .map(|directory| directory.join("pyproject.toml"))
        .find(|path| path.is_file())
        .expect("Python binding source must have a project pyproject.toml in an ancestor");
    println!("cargo:rerun-if-changed={}", pyproject.display());

    let source = std::fs::read_to_string(&pyproject)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", pyproject.display()));
    let version = project_version(&source)
        .unwrap_or_else(|| panic!("no [project].version found in {}", pyproject.display()));
    println!("cargo:rustc-env=GLASSVM_PYTHON_DISTRIBUTION_VERSION={version}");
}

fn project_version(source: &str) -> Option<&str> {
    let mut in_project = false;
    for line in source.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_project = line == "[project]";
            continue;
        }
        if !in_project {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() == "version" {
            let version = value.trim().trim_matches('"').trim_matches('\'');
            if !version.is_empty() {
                return Some(version);
            }
            return None;
        }
    }
    None
}
