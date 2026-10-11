use std::{env, fs, path::PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets the manifest directory"));
    let workspace = manifest
        .ancestors()
        .nth(2)
        .expect("the crate lives two levels under the workspace");
    let icon = workspace.join("assets/windows/baseapp.ico");
    println!("cargo:rerun-if-changed={}", icon.display());

    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets the output directory"));
    let resource = out.join("baseapp-icon.rc");
    let icon = icon.to_string_lossy().replace('\\', "\\\\");
    fs::write(&resource, format!("1 ICON \"{icon}\"\n"))
        .expect("could not write the Windows icon resource");
    println!("cargo:rerun-if-changed=build.rs");

    if let Err(error) = embed_resource::compile(resource, embed_resource::NONE).manifest_optional() {
        panic!("could not embed the Windows application icon: {error}");
    }
}
