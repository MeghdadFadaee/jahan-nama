#[cfg(windows)]
fn main() {
    use std::env;
    use std::fs;
    use std::path::Path;
    use std::path::PathBuf;
    use std::process::Command;

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let resource = manifest_dir.join("app.rc");
    let compiled = out_dir.join("mci-client.res");

    println!("cargo:rerun-if-changed={}", resource.display());
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("icon.ico").display()
    );

    copy_icon_to_profile_dir(&manifest_dir, &out_dir);

    let status = Command::new("rc.exe")
        .current_dir(&manifest_dir)
        .arg("/nologo")
        .arg(format!("/fo{}", compiled.display()))
        .arg(&resource)
        .status();

    if let Ok(status) = status
        && status.success()
    {
        println!("cargo:rustc-link-arg-bins={}", compiled.display());
    }

    fn copy_icon_to_profile_dir(manifest_dir: &Path, out_dir: &Path) {
        let Some(profile_dir) = out_dir.ancestors().nth(3) else {
            return;
        };

        let source = manifest_dir.join("icon.ico");
        let destination = profile_dir.join("icon.ico");
        let _ = fs::copy(source, destination);
    }
}

#[cfg(not(windows))]
fn main() {}
