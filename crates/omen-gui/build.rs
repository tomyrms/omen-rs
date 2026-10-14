fn main() {
    // Embed an administrator UAC level into omen-hub.exe on MSVC targets so a
    // double-click triggers the UAC prompt by itself (the HP BIOS WMI
    // interface requires an elevated token, like the vendor application).
    //
    // Note: the level must be given to the linker directly (/MANIFESTUAC).
    // Supplying an XML file via /MANIFESTINPUT conflicts with the manifest
    // snippet the linker generates itself (mt.exe error c1010001).
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();

    // Application icon (Explorer, shortcuts, taskbar) from assets/omen-hub-rs.ico.
    println!("cargo:rerun-if-changed=omen-hub.rc");
    println!("cargo:rerun-if-changed=../../assets/omen-hub-rs.ico");
    if target_os == "windows" {
        embed_resource::compile("omen-hub.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("compiling the icon resource");
    }

    if target_os == "windows" && target_env == "msvc" {
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-bins=/MANIFESTUAC:level='requireAdministrator' uiAccess='false'"
        );
    }
}
