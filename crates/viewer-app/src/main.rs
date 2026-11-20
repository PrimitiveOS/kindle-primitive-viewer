use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

use viewer_kindle::Viewer;

fn main() -> Result<(), String> {
    let root = PathBuf::from("/mnt/us");
    let app_dir = root.join("viewer");
    let mut viewer = Viewer::new(root, app_dir.clone());
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match args.as_slice() {
        [] => {}
        [flag, path] if flag == OsStr::new("--open") => viewer.open_from_files(Path::new(path)),
        _ => return Err("Usage: viewer-kindle [--open FILE]".into()),
    }
    primitive_app::run(viewer, &app_dir)
}
