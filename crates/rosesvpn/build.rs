use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=ROSESVPN_BUILD_VERSION");
    let version = env::var("ROSESVPN_BUILD_VERSION")
        .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string());
    println!("cargo:rustc-env=ROSESVPN_BUILD_VERSION={version}");

    let manifest = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets the manifest directory"),
    );
    let workspace = manifest
        .ancestors()
        .nth(2)
        .expect("the crate lives two levels under the workspace");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let dll = workspace.join("resources-windows/binaries/wireguard-amd64.dll");
        println!("cargo:rerun-if-changed={}", dll.display());
        let target =
            PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets the output directory"));
        let target = target
            .ancestors()
            .nth(3)
            .expect("OUT_DIR is below target/profile");
        let bundled = target.join("resources-windows/binaries/wireguard-amd64.dll");
        fs::create_dir_all(bundled.parent().unwrap())
            .expect("could not create WireGuardNT resource directory");
        fs::copy(&dll, &bundled).expect("could not bundle the official WireGuardNT DLL");
    }
    let icon = workspace.join("assets/windows/rosesvpn.ico");
    println!("cargo:rerun-if-changed={}", icon.display());

    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets the output directory"));
    let resource = out.join("rosesvpn-icon.rc");
    let icon = icon.to_string_lossy().replace('\\', "\\\\");
    fs::write(
        &resource,
        format!(
            "1 ICON \"{icon}\"\n\
             1 VERSIONINFO\n\
             FILEVERSION 0,42,2,0\n\
             PRODUCTVERSION 0,42,2,0\n\
             FILEFLAGSMASK 0x3fL\n\
             FILEFLAGS 0\n\
             FILEOS 0x40004L\n\
             FILETYPE 0x1L\n\
             BEGIN\n\
               BLOCK \"StringFileInfo\"\n\
               BEGIN\n\
                 BLOCK \"040904B0\"\n\
                 BEGIN\n\
                   VALUE \"CompanyName\", \"RosesVPN\\0\"\n\
                   VALUE \"FileDescription\", \"RosesVPN Desktop Client\\0\"\n\
                   VALUE \"FileVersion\", \"0.42.2\\0\"\n\
                   VALUE \"InternalName\", \"RosesVPN\\0\"\n\
                   VALUE \"OriginalFilename\", \"RosesVPN.exe\\0\"\n\
                   VALUE \"ProductName\", \"RosesVPN\\0\"\n\
                   VALUE \"ProductVersion\", \"0.42.2\\0\"\n\
                 END\n\
               END\n\
               BLOCK \"VarFileInfo\"\n\
               BEGIN\n\
                 VALUE \"Translation\", 0x409, 1200\n\
               END\n\
             END\n"
        ),
    )
    .expect("could not write the Windows icon resource");
    println!("cargo:rerun-if-changed=build.rs");

    if let Err(error) = embed_resource::compile(resource, embed_resource::NONE).manifest_optional()
    {
        panic!("could not embed the Windows application icon: {error}");
    }
}
