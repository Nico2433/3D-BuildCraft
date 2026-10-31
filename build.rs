use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=packaging/windows/icon.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let icon = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"))
        .join("packaging/windows/icon.ico");
    let rc = out.join("icon.rc");
    let res = out.join("icon.res");
    let obj = out.join("icon.obj");
    fs::write(
        &rc,
        format!(
            "1 ICON \"{}\"\n",
            icon.display().to_string().replace('\\', "/")
        ),
    )
    .expect("write icon resource script");
    run(Command::new("llvm-rc").arg("/fo").arg(&res).arg(&rc));
    let machine = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86_64") => "/machine:x64",
        Ok("aarch64") => "/machine:arm64",
        Ok("x86") => "/machine:x86",
        _ => panic!("unsupported Windows resource architecture"),
    };
    run(Command::new("llvm-cvtres")
        .arg(machine)
        .arg(format!("/out:{}", obj.display()))
        .arg(&res));
    println!("cargo:rustc-link-arg-bin=buildcraft3d={}", obj.display());
}

fn run(command: &mut Command) {
    let status = command.status().expect("run LLVM resource compiler");
    assert!(status.success(), "resource compiler failed: {command:?}");
}
