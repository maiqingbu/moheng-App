#[allow(dead_code)]
#[path = "src/skill_deployment.rs"]
mod skill_deployment;

use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let package_root = manifest_dir.join("../creative-skills/v1.2.0");
    let manifest_path = manifest_dir.join("resources/creative-skills-v1.2.0.deployment.json");
    println!("cargo:rerun-if-changed={}", manifest_path.display());
    match skill_deployment::verify_source_package(&package_root, &manifest_path) {
        Ok(deployment) => {
            for relative in deployment.file_paths() {
                println!(
                    "cargo:rerun-if-changed={}",
                    package_root.join(relative).display()
                );
            }
            println!(
                "cargo:rustc-env=CREATIVE_SKILLS_DEPLOYMENT_MANIFEST_SHA256={}",
                deployment.manifest_digest
            );
        }
        Err(error) => panic!("PRE-01 skill deployment verification failed: {error}"),
    }
    tauri_build::build()
}
