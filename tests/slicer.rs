use buildcraft3d::slicer::{detect_in, from_path};
use std::path::PathBuf;

#[test]
fn finds_a_slicer_on_path_and_names_a_picked_one() {
    let dir = tempfile::tempdir().unwrap();
    let exe = if cfg!(windows) { ".exe" } else { "" };
    let prusa = dir.path().join(format!("prusa-slicer{exe}"));
    std::fs::write(&prusa, b"").unwrap();
    let found = detect_in(&[dir.path().to_path_buf()], &[]);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "PrusaSlicer");
    assert_eq!(found[0].path, prusa);

    assert!(detect_in(&[PathBuf::from("/nonexistent")], &[]).is_empty());
    assert_eq!(
        from_path(&PathBuf::from("/opt/orca-slicer")).name,
        "OrcaSlicer"
    );
    assert_eq!(
        from_path(&PathBuf::from("C:/x/Bambu Studio.exe")).name,
        "Bambu Studio"
    );
    assert_eq!(from_path(&PathBuf::from("/opt/myslicer")).name, "myslicer");
}

#[cfg(unix)]
#[test]
fn open_starts_the_slicer_with_the_model() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("arg.txt");
    let fake = dir.path().join("fake-slicer");
    std::fs::write(
        &fake,
        format!("#!/bin/sh\necho \"$1\" > '{}'\n", out.display()),
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let model = dir.path().join("scene.3mf");
    buildcraft3d::slicer::open(&fake, &model).unwrap();
    for _ in 0..100 {
        if let Ok(s) = std::fs::read_to_string(&out) {
            if !s.is_empty() {
                assert_eq!(s.trim(), model.display().to_string());
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("the slicer was not started");
}
