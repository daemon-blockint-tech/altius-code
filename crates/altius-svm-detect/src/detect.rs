use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::Deserialize;

use crate::anchor_manifest::AnchorToml;
use crate::cluster::Cluster;
use crate::error::DetectError;
use crate::framework::Framework;
use crate::project::{ProgramInfo, SvmProject};
use crate::toolchain::Toolchain;

const SKIPPED_DIR_NAMES: [&str; 5] = ["target", "node_modules", ".git", ".anchor", "test-ledger"];
const MAX_WALK_DEPTH: usize = 6;

/// Detect what kind of SVM project (if any) lives at `root`, following the
/// ordered rules from the Phase 0 spec: Anchor first (an `Anchor.toml`
/// workspace also carries `solana-program` transitively, so it must win
/// over the native/Pinocchio checks), then Pinocchio, then native.
///
/// Returns `Ok(None)` when `root` is not an SVM project at all — that is
/// not an error, it just means SVM features stay inactive for this
/// directory.
pub fn detect(root: &Path) -> Result<Option<SvmProject>, DetectError> {
    if let Some(project) = detect_anchor(root)? {
        return Ok(Some(project));
    }
    detect_cargo_based(root)
}

fn detect_anchor(root: &Path) -> Result<Option<SvmProject>, DetectError> {
    let manifest_path = root.join("Anchor.toml");
    if !manifest_path.is_file() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&manifest_path).map_err(|source| DetectError::Io {
        path: manifest_path.clone(),
        source,
    })?;
    let manifest = AnchorToml::parse(&contents).map_err(|source| DetectError::Toml {
        path: manifest_path,
        source,
    })?;

    let programs = manifest
        .program_entries()
        .into_iter()
        .map(|(name, program_id)| {
            let path = root.join("programs").join(&name);
            ProgramInfo {
                name,
                path,
                program_id: Some(program_id),
            }
        })
        .collect();

    let default_cluster = manifest
        .provider
        .as_ref()
        .and_then(|p| p.cluster.as_deref())
        .and_then(|c| Cluster::from_str(c).ok())
        .unwrap_or_default();

    Ok(Some(SvmProject {
        framework: Framework::Anchor,
        programs,
        toolchain: Toolchain::probe(),
        default_cluster,
    }))
}

#[derive(Debug, Deserialize)]
struct CargoManifest {
    package: Option<CargoPackage>,
    lib: Option<CargoLib>,
    #[serde(default)]
    dependencies: toml::value::Table,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    name: String,
}

#[derive(Debug, Deserialize)]
struct CargoLib {
    #[serde(rename = "crate-type", default)]
    crate_type: Vec<String>,
}

fn detect_cargo_based(root: &Path) -> Result<Option<SvmProject>, DetectError> {
    let manifest_paths = find_cargo_manifests(root)?;

    let mut framework: Option<Framework> = None;
    let mut programs = Vec::new();

    for manifest_path in manifest_paths {
        let contents = fs::read_to_string(&manifest_path).map_err(|source| DetectError::Io {
            path: manifest_path.clone(),
            source,
        })?;
        let manifest: CargoManifest =
            toml::from_str(&contents).map_err(|source| DetectError::Toml {
                path: manifest_path.clone(),
                source,
            })?;

        let is_program_crate = manifest
            .lib
            .as_ref()
            .map(|lib| lib.crate_type.iter().any(|t| t == "cdylib"))
            .unwrap_or(false);
        if !is_program_crate {
            continue;
        }

        let this_framework = if manifest.dependencies.contains_key("pinocchio") {
            Some(Framework::Pinocchio)
        } else if manifest.dependencies.contains_key("solana-program")
            || manifest.dependencies.contains_key("solana-sdk")
        {
            Some(Framework::Native)
        } else {
            None
        };

        let Some(this_framework) = this_framework else {
            continue;
        };

        // Pinocchio takes precedence if a workspace somehow mixes both;
        // otherwise keep whatever we've already settled on.
        framework = Some(match (framework, this_framework) {
            (Some(Framework::Pinocchio), _) => Framework::Pinocchio,
            (_, Framework::Pinocchio) => Framework::Pinocchio,
            (Some(existing), _) => existing,
            (None, new) => new,
        });

        let crate_dir = manifest_path.parent().unwrap_or(root).to_path_buf();
        let name = manifest
            .package
            .map(|p| p.name)
            .unwrap_or_else(|| crate_dir.display().to_string());
        let program_id = scrape_declared_program_id(&crate_dir);

        programs.push(ProgramInfo {
            name,
            path: crate_dir,
            program_id,
        });
    }

    let Some(framework) = framework else {
        return Ok(None);
    };

    Ok(Some(SvmProject {
        framework,
        programs,
        toolchain: Toolchain::probe(),
        default_cluster: Cluster::default(),
    }))
}

fn find_cargo_manifests(root: &Path) -> Result<Vec<PathBuf>, DetectError> {
    let mut out = Vec::new();
    walk_for_manifests(root, 0, &mut out)?;
    Ok(out)
}

fn walk_for_manifests(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) -> Result<(), DetectError> {
    if depth > MAX_WALK_DEPTH {
        return Ok(());
    }
    let entries = fs::read_dir(dir).map_err(|source| DetectError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| DetectError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if SKIPPED_DIR_NAMES.contains(&dir_name) {
                continue;
            }
            walk_for_manifests(&path, depth + 1, out)?;
        } else if path.file_name().and_then(|n| n.to_str()) == Some("Cargo.toml") {
            out.push(path);
        }
    }
    Ok(())
}

/// Best-effort scrape of `declare_id!("...")` from `src/lib.rs`. This is a
/// plain substring search rather than a syntax-aware scan: good enough to
/// surface a program id for the common case, not a substitute for parsing
/// the manifest/IDL once one exists.
fn scrape_declared_program_id(crate_dir: &Path) -> Option<String> {
    let lib_rs = fs::read_to_string(crate_dir.join("src").join("lib.rs")).ok()?;
    let marker = "declare_id!(\"";
    let start = lib_rs.find(marker)? + marker.len();
    let rest = &lib_rs[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn detects_anchor_project() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("Anchor.toml"),
            r#"
                [programs.localnet]
                my_program = "Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS"

                [provider]
                cluster = "devnet"
                wallet = "~/.config/solana/id.json"
            "#,
        );

        let project = detect(dir.path()).unwrap().expect("should detect anchor");
        assert_eq!(project.framework, Framework::Anchor);
        assert_eq!(project.default_cluster, Cluster::Devnet);
        assert_eq!(project.programs.len(), 1);
        assert_eq!(project.programs[0].name, "my_program");
        assert_eq!(
            project.programs[0].program_id.as_deref(),
            Some("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS")
        );
    }

    #[test]
    fn detects_pinocchio_project() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("Cargo.toml"),
            r#"
                [package]
                name = "my-pinocchio-program"

                [lib]
                crate-type = ["cdylib", "lib"]

                [dependencies]
                pinocchio = "0.5"
            "#,
        );

        let project = detect(dir.path())
            .unwrap()
            .expect("should detect pinocchio");
        assert_eq!(project.framework, Framework::Pinocchio);
        assert_eq!(project.programs.len(), 1);
        assert_eq!(project.programs[0].name, "my-pinocchio-program");
    }

    #[test]
    fn detects_native_workspace_with_multiple_programs() {
        let dir = tempfile::tempdir().unwrap();
        // Root is a bare cargo workspace manifest with no [package]/[lib].
        write(
            &dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"programs/*\"]\n",
        );
        write(
            &dir.path().join("programs/foo/Cargo.toml"),
            r#"
                [package]
                name = "foo"

                [lib]
                crate-type = ["cdylib", "lib"]

                [dependencies]
                solana-program = "1.18"
            "#,
        );
        write(
            &dir.path().join("programs/foo/src/lib.rs"),
            r#"declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS");"#,
        );
        write(
            &dir.path().join("programs/bar/Cargo.toml"),
            r#"
                [package]
                name = "bar"

                [lib]
                crate-type = ["cdylib", "lib"]

                [dependencies]
                solana-sdk = "1.18"
            "#,
        );

        let project = detect(dir.path()).unwrap().expect("should detect native");
        assert_eq!(project.framework, Framework::Native);
        assert_eq!(project.default_cluster, Cluster::Localnet);
        let mut names: Vec<_> = project.programs.iter().map(|p| p.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["bar", "foo"]);
        let foo = project.programs.iter().find(|p| p.name == "foo").unwrap();
        assert_eq!(
            foo.program_id.as_deref(),
            Some("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS")
        );
    }

    #[test]
    fn returns_none_for_non_svm_directory() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("README.md"), "just a readme");
        assert_eq!(detect(dir.path()).unwrap(), None);
    }

    #[test]
    fn ignores_target_directory_during_walk() {
        let dir = tempfile::tempdir().unwrap();
        // A stray build artifact under target/ must not be picked up as a
        // program crate.
        write(
            &dir.path().join("target/debug/build/some-crate/Cargo.toml"),
            r#"
                [package]
                name = "decoy"

                [lib]
                crate-type = ["cdylib"]

                [dependencies]
                solana-program = "1.18"
            "#,
        );
        assert_eq!(detect(dir.path()).unwrap(), None);
    }
}
