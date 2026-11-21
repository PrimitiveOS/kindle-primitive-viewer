use primitive_app::{HEIGHT, WIDTH};
use primitive_ui::{
    back_button, chevron, header, keyboard_key, Canvas, Icon, Layout, Rect, TextStyle, BLACK,
    CAPTION, HAIRLINE, PAPER,
};

use crate::{
    library,
    viewer::{Fit, Page, Viewer},
};

pub(crate) const VIEWPORT: Rect = Rect::new(8, 82, 1248, 1518);
pub(crate) const ROW_HEIGHT: i32 = 112;
pub(crate) const VISIBLE_ROWS: usize = 11;
const ROW_TOP: i32 = 345;
const OPEN: Rect = Rect::new(40, 175, 1184, 104);
const UP: Rect = Rect::new(40, 176, 210, 102);
const READER_DONE: Rect = Rect::new(0, 0, 134, 82);
const VIEW_MENU: Rect = Rect::new(1112, 0, 152, 82);
const JUMP: Rect = Rect::new(452, 1600, 360, 80);
const ZOOM_OUT: Rect = Rect::new(1024, 1600, 112, 80);
const ZOOM_IN: Rect = Rect::new(1136, 1600, 112, 80);
const VIEW_BOX: Rect = Rect::new(801, 99, 421, 334);
const VIEW_ACTIONS: [(&str, Action); 3] = [
    ("Fit screen", Action::Fit(Fit::Screen)),
    ("Fit width", Action::Fit(Fit::Width)),
    ("Rotate", Action::Rotate),
];
const JUMP_BOX: Rect = Rect::new(120, 515, 1024, 400);
const CANCEL: Rect = Rect::new(875, 541, 226, 100);
const NOTICE: Rect = Rect::new(125, 480, 1014, 560);
const NOTICE_OK: Rect = Rect::new(462, 895, 340, 100);
const FOLDER: Icon = Icon {
    stroke: &["M2 6 H9 L11 8 H22 V20 H2 Z"],
    fill: &[],
};
const FILE: Icon = Icon {
    stroke: &["M5 2 H14 L19 7 V22 H5 Z M14 2 V7 H19 M8 12 H16 M8 16 H16"],
    fill: &[],
};

#[derive(Clone, Copy)]
pub(crate) enum Action {
    None,
    Exit,
    Browse,
    Up,
    Recent(usize),
    Entry(usize),
    Fit(Fit),
    Zoom(f32),
    Rotate,
    ViewMenu,
    CloseViewMenu,
    Jump,
    Cancel,
    Key(primitive_ui::Key),
    ClearNotice,
}

fn row_rect(index: usize) -> Rect {
    Rect::new(40, ROW_TOP + index as i32 * ROW_HEIGHT, 1184, ROW_HEIGHT)
}

fn view_row(index: usize) -> Rect {
    Rect::new(
        VIEW_BOX.x + 8,
        VIEW_BOX.y + 8 + index as i32 * 106,
        VIEW_BOX.w - 16,
        106,
    )
}

fn control_ink(canvas: &mut Canvas, area: Rect) -> u8 {
    if canvas.control_pressed(area, false) {
        canvas.fill(area.inset(6), BLACK);
        PAPER
    } else {
        BLACK
    }
}

fn flat_label(canvas: &mut Canvas, area: Rect, title: &str, bold: bool) {
    let ink = control_ink(canvas, area);
    let style = TextStyle::new(31).center().ink(ink);
    canvas.text(
        Rect::new(area.x, area.y + 20, area.w, area.h - 20),
        title,
        if bold { style.bold() } else { style },
    );
}

fn zoom_mark(canvas: &mut Canvas, area: Rect, plus: bool) {
    let ink = control_ink(canvas, area);
    let center = (area.x + area.w / 2, area.y + area.h / 2);
    canvas.fill(Rect::new(center.0 - 17, center.1 - 2, 34, 4), ink);
    if plus {
        canvas.fill(Rect::new(center.0 - 2, center.1 - 17, 4, 34), ink);
    }
}

pub(crate) fn row_count(app: &Viewer) -> usize {
    match app.page {
        Page::Recent => app.history.len(),
        Page::Browse => app.entries.len(),
        Page::Reader => 0,
    }
}

fn row(canvas: &mut Canvas, index: usize, title: &str, detail: &str, folder: bool) {
    let area = row_rect(index);
    let pressed = canvas.control_pressed(area, false);
    let ink = if pressed { PAPER } else { BLACK };
    if pressed {
        canvas.fill(area.inset(2), BLACK);
    }
    canvas.icon(
        Rect::new(70, area.y + 29, 48, 48),
        if folder { FOLDER } else { FILE },
        ink,
    );
    canvas.text(
        Rect::new(140, area.y + 13, 960, 50),
        title,
        TextStyle::new(37).bold().ink(ink),
    );
    canvas.text(
        Rect::new(140, area.y + 65, 960, 36),
        detail,
        if pressed { CAPTION.ink(PAPER) } else { CAPTION },
    );
    chevron(canvas, Rect::new(1151, area.y + 36, 42, 42), ink);
    canvas.fill(Rect::new(70, area.bottom() - 2, 1100, 2), HAIRLINE);
}

pub(crate) fn draw(canvas: &mut Canvas, app: &Viewer) {
    match app.page {
        Page::Recent => {
            header(canvas, "Viewer", "Exit");
            canvas.button(OPEN, "Open", false, false);
            canvas.text(
                Rect::new(44, 302, 800, 42),
                "Recent",
                TextStyle::new(30).bold(),
            );
            for (index, recent) in app
                .history
                .iter()
                .skip(app.first)
                .take(VISIBLE_ROWS)
                .enumerate()
            {
                let detail = if recent.page == 0 {
                    recent
                        .path
                        .parent()
                        .map_or_else(String::new, library::title)
                } else {
                    format!("Page {}", recent.page + 1)
                };
                row(canvas, index, &library::title(&recent.path), &detail, false);
            }
            if app.history.is_empty() {
                canvas.text(
                    Rect::new(44, 377, 1120, 100),
                    "Open a PDF, JPEG, or PNG.",
                    CAPTION,
                );
            }
        }
        Page::Browse => {
            header(canvas, "Open file", "Back");
            let can_go_up = !app.folder.as_os_str().is_empty();
            if can_go_up {
                canvas.button(UP, "Up", false, false);
            }
            let left = if can_go_up { 285 } else { 40 };
            canvas.text(
                Rect::new(left, 197, 1190 - left, 84),
                &library::title(&app.folder),
                TextStyle::new(43).bold(),
            );
            for (index, entry) in app
                .entries
                .iter()
                .skip(app.first)
                .take(VISIBLE_ROWS)
                .enumerate()
            {
                let detail = if entry.folder {
                    "Folder".into()
                } else {
                    entry
                        .path
                        .extension()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_uppercase()
                };
                row(
                    canvas,
                    index,
                    &library::title(&entry.path),
                    &detail,
                    entry.folder,
                );
            }
            if app.entries.is_empty() {
                canvas.text(
                    Rect::new(44, 377, 1120, 96),
                    "No supported files in this folder.",
                    CAPTION,
                );
            }
        }
        Page::Reader => {
            if let Some(reader) = &app.reader {
                flat_label(canvas, READER_DONE, "Exit", true);
                flat_label(canvas, VIEW_MENU, "View", false);
                canvas.text(
                    Rect::new(153, 19, 945, 56),
                    &reader.name,
                    TextStyle::new(34).bold(),
                );
                canvas.fill(Rect::new(0, VIEWPORT.y - 1, WIDTH as i32, 1), HAIRLINE);
                canvas.fill(Rect::new(0, VIEWPORT.bottom(), WIDTH as i32, 1), HAIRLINE);
                if let Some(error) = &reader.error {
                    canvas.text(
                        Rect::new(100, 650, 1064, 260),
                        error,
                        TextStyle::new(40).center().lines(4),
                    );
                } else if let Some(picture) = &reader.frame {
                    canvas.picture(VIEWPORT, picture.clone());
                }
                if reader.multipage {
                    flat_label(
                        canvas,
                        JUMP,
                        &format!("{} / {}", reader.page + 1, reader.count),
                        true,
                    );
                }
                zoom_mark(canvas, ZOOM_OUT, false);
                zoom_mark(canvas, ZOOM_IN, true);
                if app.view_menu {
                    canvas.popup(VIEW_BOX);
                    for (index, (name, action)) in VIEW_ACTIONS.into_iter().enumerate() {
                        let area = view_row(index);
                        let ink = control_ink(canvas, area);
                        canvas.text(
                            Rect::new(area.x + 24, area.y + 29, area.w - 90, 64),
                            name,
                            TextStyle::new(36).ink(ink),
                        );
                        if matches!(action, Action::Fit(fit) if reader.fit == fit) {
                            canvas.circle(Rect::new(area.right() - 52, area.y + 43, 21, 21), ink);
                        }
                        if index + 1 < VIEW_ACTIONS.len() {
                            canvas.fill(
                                Rect::new(area.x + 16, area.bottom() - 1, area.w - 32, 1),
                                HAIRLINE,
                            );
                        }
                    }
                }
            }
        }
    }
    let count = row_count(app);
    if count > VISIBLE_ROWS {
        let range = format!(
            "{}–{} of {count}",
            app.first + 1,
            (app.first + VISIBLE_ROWS).min(count)
        );
        canvas.text(Rect::new(40, 1614, 1184, 40), &range, CAPTION.center());
    }
    if let (Some(editor), Some(reader)) = (&app.jump, &app.reader) {
        canvas.popup(JUMP_BOX);
        canvas.text(
            Rect::new(165, 550, 690, 80),
            "Go to page",
            TextStyle::new(50).bold(),
        );
        canvas.button(CANCEL, "Cancel", false, false);
        canvas.single_line_field(Rect::new(175, 661, 900, 118), editor, "Page number", false);
        let message = if app.jump_notice {
            format!("Enter a page from 1 to {}.", reader.count)
        } else {
            format!("1–{}", reader.count)
        };
        canvas.text(Rect::new(175, 800, 900, 60), &message, CAPTION);
        canvas.keyboard(editor.layer, "Go", None);
    }
    if let Some(notice) = &app.notice {
        canvas.popup(NOTICE);
        canvas.text(
            Rect::new(180, 537, 900, 85),
            "Viewer",
            TextStyle::new(50).bold().center(),
        );
        canvas.text(
            Rect::new(180, 666, 900, 187),
            notice,
            TextStyle::new(35).center().lines(4),
        );
        canvas.button(NOTICE_OK, "OK", true, false);
    }
}

pub(crate) fn hit(app: &Viewer, x: f32, y: f32) -> Action {
    if app.notice.is_some() {
        return if NOTICE_OK.contains(x, y) || !NOTICE.contains(x, y) {
            Action::ClearNotice
        } else {
            Action::None
        };
    }
    if let Some(editor) = &app.jump {
        let keyboard = Layout::new(WIDTH, HEIGHT, 4, 5);
        if CANCEL.contains(x, y) {
            return Action::Cancel;
        }
        if let Some(key) = keyboard_key(&keyboard, editor, "Go", x, y) {
            return Action::Key(key);
        }
        return if !JUMP_BOX.contains(x, y) {
            Action::Cancel
        } else {
            Action::None
        };
    }
    if app.page != Page::Reader && back_button(WIDTH as i32).contains(x, y) {
        return Action::Exit;
    }
    match app.page {
        Page::Recent if OPEN.contains(x, y) => return Action::Browse,
        Page::Browse if !app.folder.as_os_str().is_empty() && UP.contains(x, y) => {
            return Action::Up
        }
        Page::Reader => {
            if app.view_menu {
                for (index, (_, action)) in VIEW_ACTIONS.into_iter().enumerate() {
                    if view_row(index).contains(x, y) {
                        return action;
                    }
                }
                return Action::CloseViewMenu;
            }
            for (area, action) in [
                (READER_DONE, Action::Exit),
                (VIEW_MENU, Action::ViewMenu),
                (ZOOM_OUT, Action::Zoom(0.8)),
                (ZOOM_IN, Action::Zoom(1.25)),
            ] {
                if area.contains(x, y) {
                    return action;
                }
            }
            if app.reader.as_ref().is_some_and(|reader| reader.multipage) && JUMP.contains(x, y) {
                return Action::Jump;
            }
        }
        _ => {}
    }
    for index in 0..row_count(app).saturating_sub(app.first).min(VISIBLE_ROWS) {
        if row_rect(index).contains(x, y) {
            return match app.page {
                Page::Recent => Action::Recent(app.first + index),
                Page::Browse => Action::Entry(app.first + index),
                Page::Reader => Action::None,
            };
        }
    }
    Action::None
}
