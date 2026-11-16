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
    // Without LLVM's resource tools (as on some CI runners) the exe just has no icon.
    let (Some(llvm_rc), Some(llvm_cvtres)) = (tool("llvm-rc"), tool("llvm-cvtres")) else {
        println!("cargo:warning=llvm-rc/llvm-cvtres not found; the exe gets no icon");
        return;
    };
    run(Command::new(llvm_rc).arg("/fo").arg(&res).arg(&rc));
    let machine = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86_64") => "/machine:x64",
        Ok("aarch64") => "/machine:arm64",
        Ok("x86") => "/machine:x86",
        _ => panic!("unsupported Windows resource architecture"),
    };
    run(Command::new(llvm_cvtres)
        .arg(machine)
        .arg(format!("/out:{}", obj.display()))
        .arg(&res));
    println!("cargo:rustc-link-arg-bin=buildcraft3d={}", obj.display());
}

/// `name` on the PATH, or in LLVM's usual Windows install folder.
fn tool(name: &str) -> Option<PathBuf> {
    let on_path = Command::new(name).arg("/?").output().is_ok();
    if on_path {
        return Some(PathBuf::from(name));
    }
    let program_files = env::var_os("ProgramFiles").map(PathBuf::from)?;
    let path = program_files.join("LLVM/bin").join(format!("{name}.exe"));
    path.is_file().then_some(path)
}

fn run(command: &mut Command) {
    let status = command.status().expect("run LLVM resource compiler");
    assert!(status.success(), "resource compiler failed: {command:?}");
}
