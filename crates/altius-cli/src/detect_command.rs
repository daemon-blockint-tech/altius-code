use std::path::Path;

use altius_svm_detect::detect;

use crate::error::CliError;

pub fn run_detect(project: &Path) -> Result<(), CliError> {
    match detect(project)? {
        None => {
            println!(
                "{} is not an SVM project (no Anchor.toml, no cargo-based program crate found)",
                project.display()
            );
        }
        Some(svm_project) => {
            println!("framework: {:?}", svm_project.framework);
            println!("default cluster: {}", svm_project.default_cluster);
            println!("programs:");
            for program in &svm_project.programs {
                let id = program
                    .program_id
                    .as_deref()
                    .unwrap_or("(no program id declared)");
                println!("  {} — {} — {}", program.name, program.path.display(), id);
            }
            println!("toolchain: {:?}", svm_project.toolchain);
        }
    }
    Ok(())
}
