use std::{
    fs, io,
    path::{Path, PathBuf},
};

use primitive_app::{font_paths, frame, write_png, App, Change, HEIGHT, WIDTH};
use primitive_ui::{Canvas, Input};
use viewer_kindle::Viewer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let [root, output, pdf, jpeg, png] = args.as_slice() else {
        return Err(
            "Usage: viewer-desktop STORAGE OUTPUT PDF JPEG PNG (paths under STORAGE)".into(),
        );
    };
    fs::create_dir_all(output)?;
    let app_dir = output.join(format!("session-{}", std::process::id()));
    fs::create_dir(&app_dir)?;
    let fonts = std::env::var_os("PRIMITIVE_FONTS_DIR").map_or_else(
        || {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../doannt-kindle-launcher/assets/fonts")
        },
        PathBuf::from,
    );
    let mut canvas = Canvas::new(WIDTH, HEIGHT, &font_paths(&fonts)).map_err(io::Error::other)?;

    let mut app = Viewer::new(root.clone(), app_dir.clone());
    app.open_from_files(&root.join(pdf));
    let first = frame(&app, &mut canvas, false);
    write_png(&output.join("viewer-pdf.png"), WIDTH, HEIGHT, &first)?;
    assert_eq!(app.input(Input::PageForward), Change::Partial);
    assert_ne!(
        frame(&app, &mut canvas, false),
        first,
        "Page forward should scroll the document"
    );
    assert_eq!(app.input(Input::PageBack), Change::Partial);
    assert_eq!(
        frame(&app, &mut canvas, false),
        first,
        "Page back should reverse the scroll"
    );
    assert_eq!(
        app.input(Input::Swipe {
            dx: 0.0,
            dy: -1320.0,
            start_y: 700.0
        }),
        Change::Partial
    );
    let scrolled = frame(&app, &mut canvas, false);
    assert_ne!(first, scrolled);
    write_png(&output.join("viewer-scroll.png"), WIDTH, HEIGHT, &scrolled)?;
    app.save()?;

    let mut app = Viewer::new(root.clone(), app_dir.clone());
    assert_eq!(app.input(Input::Tap(350.0, 399.0)), Change::Partial);
    assert_eq!(
        frame(&app, &mut canvas, false),
        scrolled,
        "Recent should resume the same part of the PDF"
    );
    assert_eq!(app.input(Input::Tap(68.0, 41.0)), Change::Partial);

    for (name, path) in [("viewer-photo.png", jpeg), ("viewer-image.png", png)] {
        let mut app = Viewer::new(root.clone(), app_dir.clone());
        app.open_from_files(&root.join(path));
        let original = frame(&app, &mut canvas, false);
        assert_eq!(app.input(Input::Tap(1190.0, 1640.0)), Change::Partial);
        assert_ne!(
            original,
            frame(&app, &mut canvas, false),
            "Zoom should update the content"
        );
        assert_eq!(app.input(Input::Tap(1185.0, 41.0)), Change::Partial);
        assert_eq!(app.input(Input::Tap(1000.0, 370.0)), Change::Partial);
        let rotated = frame(&app, &mut canvas, false);
        assert_ne!(original, rotated, "Rotate should update the content");
        write_png(&output.join(name), WIDTH, HEIGHT, &original)?;
        app.save()?;
    }

    let mut app = Viewer::new(root.clone(), app_dir.clone());
    assert_eq!(app.input(Input::Tap(510.0, 235.0)), Change::Partial);
    write_png(
        &output.join("viewer-open.png"),
        WIDTH,
        HEIGHT,
        &frame(&app, &mut canvas, false),
    )?;
    assert_eq!(app.input(Input::Tap(1100.0, 85.0)), Change::Partial);
    write_png(
        &output.join("viewer-recent.png"),
        WIDTH,
        HEIGHT,
        &frame(&app, &mut canvas, false),
    )?;
    fs::remove_file(app_dir.join("reading.json"))?;
    fs::remove_dir(app_dir)?;
    Ok(())
}
