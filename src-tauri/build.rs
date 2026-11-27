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
    embed_skills_v2(&manifest_dir.join("../creative-skills/v2.0.0/skills"));
    tauri_build::build()
}

/// Embeds every v2 `SKILL.md` into the binary so installed builds read
/// exactly the reviewed text.
fn embed_skills_v2(skills_root: &std::path::Path) {
    println!("cargo:rerun-if-changed={}", skills_root.display());
    let mut entries = std::fs::read_dir(skills_root)
        .unwrap_or_else(|error| panic!("v2 skills directory unreadable: {error}"))
        .map(|entry| entry.expect("v2 skills entry").path().join("SKILL.md"))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    entries.sort();
    let mut generated = String::from("pub static SKILL_FILES: &[&str] = &[\n");
    for path in &entries {
        println!("cargo:rerun-if-changed={}", path.display());
        generated.push_str(&format!(
            "    include_str!({:?}),\n",
            path.display().to_string()
        ));
    }
    generated.push_str("];\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(out.join("skills_v2_embedded.rs"), generated)
        .unwrap_or_else(|error| panic!("v2 skills embedding failed: {error}"));
}
