use std::{
    io,
    path::{Path, PathBuf},
};

use primitive_app::{App, Change};
use primitive_ui::{Canvas, Editor, GrayImage, Input, Key, Layer};

use crate::{
    library::{self, Entry, Recent},
    pdf::{Pdf, RenderRequest, MAX_PDF_BYTES},
    picture::Picture,
    ui::{self, Action},
};

enum Content {
    Pdf(Pdf),
    Image(Picture),
}

impl Content {
    fn read(root: &Path, path: &Path) -> Result<Self, String> {
        let bytes = primitive_app::with_storage(|| library::read(root, path, MAX_PDF_BYTES))
            .map_err(|error| {
                if error.kind() == io::ErrorKind::NotFound {
                    "This file is no longer on the Kindle.".into()
                } else {
                    error.to_string()
                }
            })?;
        let header = &bytes[..bytes.len().min(1024)];
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") || bytes.starts_with(b"\xff\xd8\xff") {
            Picture::open(bytes).map(Self::Image)
        } else if header.windows(5).any(|word| word == b"%PDF-") {
            Pdf::open(bytes)
                .map(Self::Pdf)
                .map_err(|error| error.to_string())
        } else {
            Err(
                "This file is damaged or is outside the supported PDF, JPEG, and PNG formats."
                    .into(),
            )
        }
    }

    fn page_count(&self) -> usize {
        match self {
            Self::Pdf(pdf) => pdf.page_count(),
            Self::Image(_) => 1,
        }
    }

    fn size(&self, page: usize) -> Result<(f32, f32), String> {
        match self {
            Self::Pdf(pdf) => pdf.page_size(page).map_err(|error| error.to_string()),
            Self::Image(image) => Ok((image.width() as f32, image.height() as f32)),
        }
    }
}

const PAGE_GAP: u32 = 16;
const PAPER_MIN_HEIGHT: u32 = 176;
const DOCUMENT_BACKGROUND: u8 = 230;
const MAX_PROJECTED_SIDE: f32 = 32_000.0;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Fit {
    Screen,
    Width,
    Custom,
}

#[derive(Clone, Copy)]
struct Viewport {
    mode: Fit,
    zoom_basis: Fit,
    turns: u8,
    zoom: f32,
    x: f32,
    y: f32,
}

impl Viewport {
    fn new(mode: Fit) -> Self {
        Self {
            mode,
            zoom_basis: mode,
            turns: 0,
            zoom: 1.0,
            x: 0.0,
            y: 0.0,
        }
    }

    fn projection(&self, size: (f32, f32)) -> (f32, f32, f32) {
        let (w, h) = if self.turns % 2 == 0 {
            size
        } else {
            (size.1, size.0)
        };
        let fit_width = ui::VIEWPORT.w as f32 / w;
        let fit = if self.zoom_basis == Fit::Width {
            fit_width
        } else {
            fit_width.min(ui::VIEWPORT.h as f32 / h)
        };
        let scale = (fit * self.zoom).min(MAX_PROJECTED_SIDE / w.max(h));
        (w * scale, h * scale, scale)
    }

    fn picture_request(&self, size: (f32, f32)) -> RenderRequest {
        let (width, height) = (ui::VIEWPORT.w as u32, ui::VIEWPORT.h as u32);
        let (w, h, scale) = self.projection(size);
        let place = |area: u32, content: f32, offset: f32| {
            let empty = area as f32 - content;
            if empty >= 0.0 {
                empty / 2.0
            } else {
                offset.clamp(empty, 0.0)
            }
        };
        RenderRequest {
            width,
            height,
            scale,
            x: place(width, w, self.x),
            y: place(height, h, self.y),
            turns: self.turns,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ScrollAnchor {
    page: usize,
    offset: u32,
}

struct Paper {
    start: u32,
    height: u32,
    padding: u32,
    width: f32,
    scale: f32,
}

struct PdfFlow {
    pages: Vec<Paper>,
    anchor: ScrollAnchor,
    max_top: u32,
}

impl PdfFlow {
    fn new(pdf: &Pdf, view: Viewport) -> Self {
        Self::layout(
            (0..pdf.page_count()).map(|page| pdf.page_size(page).ok()),
            view,
        )
    }

    fn layout(sizes: impl IntoIterator<Item = Option<(f32, f32)>>, view: Viewport) -> Self {
        let mut start = 0;
        let pages: Vec<_> = sizes
            .into_iter()
            .map(|size| {
                let (width, height, scale) = size
                    .map(|size| view.projection(size))
                    .filter(|&(w, h, scale)| {
                        w.is_finite()
                            && h.is_finite()
                            && scale.is_finite()
                            && w > 0.0
                            && h > 0.0
                            && scale > 0.0
                    })
                    .unwrap_or((ui::VIEWPORT.w as f32, ui::VIEWPORT.h as f32, 0.0));
                let content_height = (height.ceil() as u32).max(1);
                // Keep extreme landscape pages visible without shrinking or stretching their content.
                let height = content_height.max(PAPER_MIN_HEIGHT);
                let paper = Paper {
                    start,
                    height,
                    padding: (height - content_height) / 2,
                    width,
                    scale,
                };
                start += height + PAGE_GAP;
                paper
            })
            .collect();
        let last = &pages[pages.len() - 1];
        let end = last.start + last.height;
        Self {
            anchor: ScrollAnchor { page: 0, offset: 0 },
            // Short final pages can still be brought to the top by Go to page.
            max_top: end.saturating_sub(ui::VIEWPORT.h as u32).max(last.start),
            pages,
        }
    }

    fn top(&self) -> u32 {
        self.pages[self.anchor.page].start + self.anchor.offset
    }

    fn locate(&self, y: u32) -> ScrollAnchor {
        let page = self.pages.partition_point(|paper| paper.start <= y) - 1;
        let paper = &self.pages[page];
        let offset = (y - paper.start).min(paper.height + PAGE_GAP - 1);
        ScrollAnchor { page, offset }
    }

    fn position(&self, anchor: ScrollAnchor) -> (usize, u16) {
        let span = self.pages[anchor.page].height + PAGE_GAP;
        let value = (f64::from(anchor.offset) / f64::from(span) * f64::from(u16::MAX)).round();
        (anchor.page, value as u16)
    }

    fn restore(&mut self, page: usize, position: u16, viewport_y: u32) {
        let paper = &self.pages[page.min(self.pages.len() - 1)];
        let span = paper.height + PAGE_GAP;
        let offset = (f64::from(span) * f64::from(position) / f64::from(u16::MAX)).round() as u32;
        let target = (paper.start + offset.min(span - 1)).saturating_sub(viewport_y);
        self.move_to(target);
    }

    fn move_to(&mut self, top: u32) -> bool {
        let anchor = self.locate(top.min(self.max_top));
        let changed = anchor != self.anchor;
        self.anchor = anchor;
        changed
    }

    fn visible(&self) -> impl Iterator<Item = (usize, &Paper)> {
        let bottom = self.top() + ui::VIEWPORT.h as u32;
        self.pages
            .iter()
            .enumerate()
            .skip(self.anchor.page)
            .take_while(move |(_, paper)| paper.start < bottom)
    }

    fn render(&self, pdf: &Pdf, view: Viewport, frame: &mut GrayImage) -> Option<String> {
        let width = frame.width as usize;
        let height = frame.height as i32;
        let top = self.top() as i32;
        frame.pixels.fill(DOCUMENT_BACKGROUND);
        let mut drawn = false;
        let mut failure = None;
        for (index, paper) in self.visible() {
            let paper_top = paper.start as i32 - top;
            let slice_top = paper_top.max(0);
            let slice_bottom = (paper_top + paper.height as i32).min(height);
            if slice_top >= slice_bottom {
                continue;
            }
            if paper.scale <= 0.0 {
                failure = Some("This PDF page has unsupported dimensions.".into());
                continue;
            }
            let horizontal_space = width as f32 - paper.width;
            let x = if horizontal_space >= 0.0 {
                horizontal_space / 2.0
            } else {
                (horizontal_space / 2.0 + view.x).clamp(horizontal_space, 0.0)
            };
            let left = x.floor().max(0.0) as usize;
            let right = (x + paper.width).ceil().min(width as f32) as usize;
            let stride = right - left;
            if stride == 0 {
                continue;
            }
            let request = RenderRequest {
                width: stride as u32,
                height: (slice_bottom - slice_top) as u32,
                scale: paper.scale,
                x: x - left as f32,
                y: (paper_top + paper.padding as i32 - slice_top) as f32,
                turns: view.turns,
            };
            match pdf.render(index, request) {
                Ok(pixels) => {
                    for (target, source) in frame
                        .pixels
                        .chunks_exact_mut(width)
                        .skip(slice_top as usize)
                        .zip(pixels.chunks_exact(stride))
                    {
                        target[left..right].copy_from_slice(source);
                    }
                    drawn = true;
                }
                Err(error) => failure = Some(error.to_string()),
            }
        }
        if drawn {
            None
        } else {
            failure
        }
    }
}

pub(crate) struct Reader {
    path: PathBuf,
    pub name: String,
    pub page: usize,
    pub count: usize,
    pub multipage: bool,
    pub frame: Option<GrayImage>,
    pub error: Option<String>,
    pub fit: Fit,
    document: Content,
    view: Viewport,
    flow: Option<PdfFlow>,
}

impl Reader {
    fn draw_content(&mut self) {
        let started = std::time::Instant::now();
        match (&self.document, &self.flow) {
            (Content::Pdf(pdf), Some(flow)) => {
                let frame = self.frame.get_or_insert_with(|| {
                    GrayImage::blank(ui::VIEWPORT.w as u32, ui::VIEWPORT.h as u32)
                });
                self.page = flow.anchor.page;
                self.error = flow.render(pdf, self.view, frame);
            }
            (Content::Image(image), _) => {
                let request = self
                    .view
                    .picture_request((image.width() as f32, image.height() as f32));
                self.view.x = request.x;
                self.view.y = request.y;
                match image.render(request) {
                    Ok(pixels) => {
                        self.frame = Some(GrayImage {
                            width: request.width,
                            height: request.height,
                            pixels,
                        });
                        self.error = None;
                    }
                    Err(error) => {
                        self.frame = None;
                        self.error = Some(error);
                    }
                }
            }
            (Content::Pdf(_), None) => return,
        }
        self.fit = self.view.mode;
        if self.error.is_none() {
            eprintln!(
                "Viewer rendered page {}/{} in {} ms",
                self.page + 1,
                self.count,
                started.elapsed().as_millis()
            );
        }
    }

    fn reflow(&mut self, viewport_y: u32) {
        let (Content::Pdf(pdf), Some(flow)) = (&self.document, &self.flow) else {
            return;
        };
        let (page, position) = flow.position(flow.locate(flow.top() + viewport_y));
        let mut next = PdfFlow::new(pdf, self.view);
        next.restore(page, position, viewport_y);
        self.flow = Some(next);
    }

    fn position(&self) -> (usize, u16) {
        self.flow
            .as_ref()
            .map_or((0, 0), |flow| flow.position(flow.anchor))
    }

    fn fit(&mut self, fit: Fit) {
        self.view.mode = fit;
        self.view.zoom_basis = fit;
        self.view.zoom = 1.0;
        self.view.x = 0.0;
        self.view.y = 0.0;
        self.reflow(0);
        self.draw_content();
    }

    fn rotate(&mut self) {
        self.view.turns = (self.view.turns + 1) % 4;
        self.view.x = 0.0;
        self.view.y = 0.0;
        self.reflow(0);
        self.draw_content();
    }

    fn zoom(&mut self, factor: f32) {
        let Ok(size) = self.document.size(self.page) else {
            return;
        };
        let previous = self.view.picture_request(size);
        let next_zoom = (self.view.zoom * factor).clamp(0.5, 8.0);
        if next_zoom == self.view.zoom {
            return;
        }
        self.view.mode = Fit::Custom;
        self.view.zoom = next_zoom;
        let ratio = self.view.projection(size).2 / previous.scale;
        if self.flow.is_some() {
            self.view.x *= ratio;
            self.reflow(ui::VIEWPORT.h as u32 / 2);
        } else {
            let half_width = previous.width as f32 / 2.0;
            let half_height = previous.height as f32 / 2.0;
            self.view.x = half_width + (previous.x - half_width) * ratio;
            self.view.y = half_height + (previous.y - half_height) * ratio;
        }
        self.draw_content();
    }

    fn go_to(&mut self, page: usize) -> bool {
        let Some(flow) = self.flow.as_mut() else {
            return false;
        };
        let top = flow.pages[page.min(self.count - 1)].start;
        if !flow.move_to(top) {
            return false;
        }
        self.draw_content();
        true
    }

    fn scroll(&mut self, delta: i32) -> bool {
        let Some(flow) = self.flow.as_mut() else {
            return false;
        };
        if !flow.move_to(flow.top().saturating_add_signed(delta)) {
            return false;
        }
        self.draw_content();
        true
    }

    fn pan(&mut self, dx: f32, dy: f32) -> bool {
        if let Some(flow) = &self.flow {
            if dy.abs() >= dx.abs() {
                return self.scroll((-dy).round() as i32);
            }
            let widest = flow
                .visible()
                .map(|(_, paper)| paper.width)
                .fold(0.0, f32::max);
            let limit = (widest - ui::VIEWPORT.w as f32).max(0.0) / 2.0;
            if limit <= 0.5 {
                return false;
            }
            let next = (self.view.x + dx).clamp(-limit, limit);
            if next.round() == self.view.x.round() {
                return false;
            }
            self.view.x = next;
        } else {
            let Ok(size) = self.document.size(self.page) else {
                return false;
            };
            let previous = self.view.picture_request(size);
            self.view.x += dx;
            self.view.y += dy;
            let next = self.view.picture_request(size);
            self.view.x = next.x;
            self.view.y = next.y;
            if next.x.round() == previous.x.round() && next.y.round() == previous.y.round() {
                return false;
            }
        }
        self.draw_content();
        true
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Page {
    Recent,
    Browse,
    Reader,
}

pub struct Viewer {
    root: PathBuf,
    history_path: PathBuf,
    pub(crate) history: Vec<Recent>,
    pub(crate) folder: PathBuf,
    pub(crate) entries: Vec<Entry>,
    pub(crate) first: usize,
    pub(crate) page: Page,
    pub(crate) reader: Option<Reader>,
    pub(crate) notice: Option<String>,
    pub(crate) jump: Option<Editor>,
    pub(crate) jump_notice: bool,
    pub(crate) view_menu: bool,
    direct: bool,
    dirty: bool,
}

impl Viewer {
    pub fn new(root: PathBuf, app_dir: PathBuf) -> Self {
        let history_path = app_dir.join("reading.json");
        let history = library::load_history(&history_path);
        let folder = PathBuf::from("documents");
        Self {
            root,
            history_path,
            history,
            folder,
            entries: Vec::new(),
            first: 0,
            page: Page::Recent,
            reader: None,
            notice: None,
            jump: None,
            jump_notice: false,
            view_menu: false,
            direct: false,
            dirty: false,
        }
    }

    pub fn open_from_files(&mut self, path: &Path) {
        match library::relative_path(&self.root, path) {
            Ok(path) => {
                self.open(path, None);
                self.direct = self.reader.is_some();
            }
            Err(_) => self.notice = Some("This file is unavailable in Kindle user storage.".into()),
        }
    }

    fn open(&mut self, path: PathBuf, view: Option<Viewport>) {
        let document = match Content::read(&self.root, &path) {
            Ok(document) => document,
            Err(error) => {
                self.notice = Some(error);
                return;
            }
        };
        let count = document.page_count();
        let (page, offset) = self
            .history
            .iter()
            .find(|recent| recent.path == path)
            .map_or((0, 0), |recent| (recent.page.min(count - 1), recent.offset));
        let multipage = matches!(document, Content::Pdf(_));
        let fit = if multipage { Fit::Width } else { Fit::Screen };
        let view = view.unwrap_or_else(|| Viewport::new(fit));
        let flow = if let Content::Pdf(pdf) = &document {
            let mut flow = PdfFlow::new(pdf, view);
            flow.restore(page, offset, 0);
            Some(flow)
        } else {
            None
        };
        let mut reader = Reader {
            name: library::title(&path),
            path,
            page,
            count,
            multipage,
            frame: None,
            error: None,
            fit,
            document,
            view,
            flow,
        };
        reader.draw_content();
        self.reader = Some(reader);
        self.page = Page::Reader;
        self.notice = None;
        self.view_menu = false;
        self.remember();
    }

    fn remember(&mut self) {
        if let Some(reader) = &self.reader {
            let (page, offset) = reader.position();
            library::remember(&mut self.history, &reader.path, page, offset);
            self.dirty = true;
        }
    }

    fn browse(&mut self, folder: PathBuf) {
        match primitive_app::with_storage(|| library::list(&self.root, &folder)) {
            Ok(entries) => {
                self.folder = folder;
                self.entries = entries;
                self.first = 0;
                self.notice = None;
            }
            Err(error) => {
                self.entries.clear();
                self.notice = Some(error.to_string());
            }
        }
        self.page = Page::Browse;
    }

    fn go_to(&mut self, page: usize) -> Change {
        if !self
            .reader
            .as_mut()
            .is_some_and(|reader| reader.go_to(page))
        {
            return Change::None;
        }
        self.remember();
        Change::Partial
    }

    fn scroll(&mut self, delta: i32) -> Change {
        if !self
            .reader
            .as_mut()
            .is_some_and(|reader| reader.scroll(delta))
        {
            return Change::None;
        }
        self.remember();
        Change::Partial
    }

    fn scroll_rows(&mut self, delta: isize) -> Change {
        let max = ui::row_count(self).saturating_sub(ui::VISIBLE_ROWS);
        let next = self.first.saturating_add_signed(delta).min(max);
        if next == self.first {
            return Change::None;
        }
        self.first = next;
        Change::Partial
    }

    fn jump_key(&mut self, key: Key) -> Change {
        let (Some(editor), Some(reader)) = (self.jump.as_mut(), self.reader.as_ref()) else {
            return Change::None;
        };
        match key {
            Key::Go => {
                let number = editor.text.iter().collect::<String>().parse::<usize>().ok();
                if let Some(number) = number.filter(|&number| (1..=reader.count).contains(&number))
                {
                    self.jump = None;
                    self.go_to(number - 1);
                } else {
                    self.jump_notice = true;
                }
            }
            Key::Char('0'..='9') | Key::Delete | Key::Left | Key::Right => {
                editor.press(key);
                self.jump_notice = false;
            }
            _ => return Change::None,
        }
        Change::Partial
    }

    fn act(&mut self, action: Action) -> Change {
        match action {
            Action::None => return Change::None,
            Action::Exit => {
                if self.page == Page::Recent || (self.direct && self.page == Page::Reader) {
                    return Change::Exit;
                }
                self.reader = None;
                self.page = Page::Recent;
                self.first = 0;
                self.notice = None;
                self.view_menu = false;
            }
            Action::Browse => {
                let folder = self.folder.clone();
                self.browse(folder);
                if self.notice.is_some() {
                    self.browse(PathBuf::new());
                }
            }
            Action::Up => self.browse(
                self.folder
                    .parent()
                    .map_or_else(PathBuf::new, Path::to_owned),
            ),
            Action::Recent(index) => {
                if let Some(path) = self.history.get(index).map(|recent| recent.path.clone()) {
                    self.open(path, None);
                }
            }
            Action::Entry(index) => {
                if let Some(entry) = self.entries.get(index) {
                    let path = entry.path.clone();
                    if entry.folder {
                        self.browse(path)
                    } else {
                        self.open(path, None)
                    };
                }
            }
            Action::Fit(fit) => {
                if let Some(reader) = &mut self.reader {
                    reader.fit(fit);
                }
                self.view_menu = false;
                self.remember();
            }
            Action::Zoom(factor) => {
                if let Some(reader) = &mut self.reader {
                    reader.zoom(factor);
                }
                self.view_menu = false;
                self.remember();
            }
            Action::Rotate => {
                if let Some(reader) = &mut self.reader {
                    reader.rotate();
                }
                self.view_menu = false;
                self.remember();
            }
            Action::ViewMenu => self.view_menu = true,
            Action::CloseViewMenu => self.view_menu = false,
            Action::Jump => {
                let mut editor = Editor::new(5);
                editor.layer = Layer::Symbols;
                self.jump = Some(editor);
                self.jump_notice = false;
            }
            Action::Cancel => self.jump = None,
            Action::Key(key) => return self.jump_key(key),
            Action::ClearNotice => self.notice = None,
        }
        Change::Partial
    }
}

impl App for Viewer {
    fn draw(&self, canvas: &mut Canvas) {
        ui::draw(canvas, self);
    }

    fn input(&mut self, input: Input) -> Change {
        if let Input::Tap(x, y) = input {
            return self.act(ui::hit(self, x, y));
        }
        if self.jump.is_some() || self.notice.is_some() || self.view_menu {
            return Change::None;
        }
        match input {
            Input::PageForward | Input::PageBack => {
                let direction = if matches!(input, Input::PageForward) {
                    1
                } else {
                    -1
                };
                if self.page == Page::Reader {
                    // Preserve a little overlap between adjacent document views.
                    let page_step = ui::VIEWPORT.h - ui::VIEWPORT.h / 10;
                    self.scroll(direction * page_step)
                } else {
                    self.scroll_rows(direction as isize * ui::VISIBLE_ROWS as isize)
                }
            }
            Input::Swipe { dx, dy, start_y } if self.page == Page::Reader => {
                if start_y < ui::VIEWPORT.y as f32 || start_y >= ui::VIEWPORT.bottom() as f32 {
                    return Change::None;
                }
                let Some(reader) = self.reader.as_mut() else {
                    return Change::None;
                };
                if reader.pan(dx, dy) {
                    self.remember();
                    Change::Partial
                } else {
                    Change::None
                }
            }
            Input::Swipe { dy, .. } if dy.abs() >= 45.0 => {
                let rows = -(dy / ui::ROW_HEIGHT as f32).round() as isize;
                let offset = if rows == 0 {
                    if dy > 0.0 {
                        -1
                    } else {
                        1
                    }
                } else {
                    rows
                };
                self.scroll_rows(offset)
            }
            _ => Change::None,
        }
    }

    fn save(&mut self) -> io::Result<()> {
        if self.dirty {
            let bytes = serde_json::to_vec(&self.history)?;
            primitive_app::save(&self.history_path, &bytes)?;
            self.dirty = false;
        }
        Ok(())
    }

    fn storage_returned(&mut self) -> io::Result<()> {
        if let Some(reader) = self.reader.take() {
            let path = reader.path.clone();
            let view = reader.view;
            drop(reader);
            self.page = Page::Recent;
            self.first = 0;
            self.jump = None;
            self.view_menu = false;
            self.open(path, Some(view));
            if self.reader.is_none() {
                self.direct = false;
            }
        } else if self.page == Page::Browse {
            self.browse(self.folder.clone());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_buttons_scroll_file_picker_and_share_bounds_with_touch() {
        let mut viewer = Viewer::new(PathBuf::new(), PathBuf::new());
        viewer.page = Page::Browse;
        viewer.entries = (0..ui::VISIBLE_ROWS * 2 + 3)
            .map(|index| Entry {
                path: format!("notes-{index}.pdf").into(),
                folder: false,
            })
            .collect();

        assert_eq!(viewer.input(Input::PageBack), Change::None);
        assert_eq!(viewer.input(Input::PageForward), Change::Partial);
        assert_eq!(viewer.first, ui::VISIBLE_ROWS);
        assert_eq!(viewer.input(Input::PageForward), Change::Partial);
        assert_eq!(viewer.first, ui::VISIBLE_ROWS + 3);
        assert_eq!(viewer.input(Input::PageForward), Change::None);
        assert_eq!(viewer.input(Input::PageBack), Change::Partial);
        assert_eq!(viewer.first, 3);
        assert_eq!(
            viewer.input(Input::Swipe {
                dx: 0.0,
                dy: ui::ROW_HEIGHT as f32,
                start_y: 700.0
            }),
            Change::Partial,
        );
        assert_eq!(viewer.first, 2);
        assert_eq!(viewer.input(Input::PageBack), Change::Partial);
        assert_eq!(viewer.first, 0);
    }

    #[test]
    fn continuous_flow_keeps_a_fractional_anchor_and_shows_both_sides_of_a_gap() {
        let width = ui::VIEWPORT.w as f32;
        let sizes = [Some((width, 1600.0)), Some((width, 605.0))];
        let mut flow = PdfFlow::layout(sizes, Viewport::new(Fit::Width));
        assert_eq!(flow.pages[1].start, 1600 + PAGE_GAP);
        assert_eq!(flow.anchor.page, 0);
        assert_eq!(flow.visible().count(), 1);
        assert!(flow.move_to(1300));
        assert_eq!(flow.anchor.page, 0);
        assert_eq!(
            flow.visible().map(|(index, _)| index).collect::<Vec<_>>(),
            [0, 1]
        );

        let (page, position) = flow.position(flow.anchor);
        let mut reopened = PdfFlow::layout(sizes, Viewport::new(Fit::Width));
        reopened.restore(page, position, 0);
        assert_eq!(reopened.top(), 1300);

        // A short last page can reach the viewport top, but scrolling farther makes no change.
        assert!(flow.move_to(u32::MAX));
        assert_eq!(flow.top(), 1600 + PAGE_GAP);
        assert_eq!(flow.anchor.page, 1);
        assert!(!flow.move_to(u32::MAX));
        assert!(flow.move_to(0));
        assert_eq!(flow.anchor.page, 0);
    }

    #[test]
    fn extremely_short_pages_keep_their_scale_and_bound_visible_slices() {
        let squares = std::iter::repeat_n(Some((ui::VIEWPORT.w as f32, 1.0)), 10_000);
        let mut flow = PdfFlow::layout(squares, Viewport::new(Fit::Width));
        assert_eq!(flow.pages.len(), 10_000);
        assert_eq!(flow.pages[0].scale, 1.0);
        assert_eq!(flow.pages[0].padding, (PAPER_MIN_HEIGHT - 1) / 2);
        assert_eq!(flow.pages[0].height, PAPER_MIN_HEIGHT);
        assert!(flow.visible().count() <= 9);
        assert!(flow.move_to(10_000));
        assert!(flow.visible().count() <= 9);
    }
}
