use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};

const USER_FOLDERS: &[&str] = &[
    "documents",
    "download",
    "downloads",
    "pictures",
    "photos",
    "screenshots",
    "music",
    "audible",
];
const MAX_HISTORY_BYTES: u64 = 64 * 1024;
const MAX_RECENTS: usize = 12;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Recent {
    pub path: PathBuf,
    pub page: usize,
    // The position within this page's virtual slot, normalized to 0..=65535.
    #[serde(default)]
    pub offset: u16,
}

pub(crate) struct Entry {
    pub path: PathBuf,
    pub folder: bool,
}

pub(crate) fn title(path: &Path) -> String {
    path.file_name().map_or_else(
        || "Kindle".into(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn user_folder(name: &str) -> bool {
    USER_FOLDERS
        .iter()
        .any(|folder| name.eq_ignore_ascii_case(folder))
}

fn supported_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            ["pdf", "jpeg", "jpg", "png"]
                .iter()
                .any(|supported| ext.eq_ignore_ascii_case(supported))
        })
}

fn allowed(relative: &Path) -> bool {
    let mut parts = relative.components();
    let Some(Component::Normal(first)) = parts.next() else {
        return relative.as_os_str().is_empty();
    };
    let Some(name) = first.to_str() else {
        return false;
    };
    if !user_folder(name) && (!supported_extension(relative) || relative.components().count() != 1)
    {
        return false;
    }
    !name.starts_with('.')
        && parts.all(|part| matches!(part, Component::Normal(name) if name.to_str().is_some_and(|s| !s.starts_with('.'))))
}

fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Choose a file from Kindle user storage.",
    )
}

fn checked(root: &Path, relative: &Path, directory: bool) -> io::Result<PathBuf> {
    if !allowed(relative) || (!directory && !supported_extension(relative)) {
        return Err(denied());
    }
    let mut path = root.to_path_buf();
    if !fs::metadata(&path)?.is_dir() {
        return Err(denied());
    }
    let mut components = relative.components().peekable();
    while let Some(part) = components.next() {
        path.push(part.as_os_str());
        let kind = fs::symlink_metadata(&path)?.file_type();
        if if directory || components.peek().is_some() {
            !kind.is_dir()
        } else {
            !kind.is_file()
        } {
            return Err(denied());
        }
    }
    Ok(path)
}

pub(crate) fn relative_path(root: &Path, path: &Path) -> io::Result<PathBuf> {
    let relative = path.strip_prefix(root).map_err(|_| denied())?;
    checked(root, relative, false)?;
    Ok(relative.to_owned())
}

pub(crate) fn list(root: &Path, folder: &Path) -> io::Result<Vec<Entry>> {
    let dir = checked(root, folder, true)?;
    let mut rows = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = folder.join(entry.file_name());
        let kind = entry.file_type()?;
        if allowed(&path) && (kind.is_dir() || (kind.is_file() && supported_extension(&path))) {
            rows.push(Entry {
                path,
                folder: kind.is_dir(),
            });
        }
    }
    rows.sort_by_cached_key(|row| {
        (
            !row.folder,
            title(&row.path).to_lowercase(),
            row.path.clone(),
        )
    });
    Ok(rows)
}

pub(crate) fn read(root: &Path, relative: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let file = File::open(checked(root, relative, false)?)?;
    let len = file.metadata()?.len();
    if len > limit as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "This file exceeds Viewer's 32 MB limit.",
        ));
    }
    let mut bytes = Vec::with_capacity(len as usize);
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "This file exceeds Viewer's 32 MB limit.",
        ));
    }
    Ok(bytes)
}

pub(crate) fn load_history(path: &Path) -> Vec<Recent> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(MAX_HISTORY_BYTES + 1).read_to_end(&mut bytes))
        .ok()
        .filter(|&size| size <= MAX_HISTORY_BYTES as usize)
        .and_then(|_| serde_json::from_slice::<Vec<Recent>>(&bytes).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|recent| allowed(&recent.path) && supported_extension(&recent.path))
        .take(MAX_RECENTS)
        .collect()
}

pub(crate) fn remember(history: &mut Vec<Recent>, path: &Path, page: usize, offset: u16) {
    history.retain(|recent| recent.path != path);
    history.insert(
        0,
        Recent {
            path: path.to_owned(),
            page,
            offset,
        },
    );
    history.truncate(MAX_RECENTS);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_without_a_scroll_offset_opens_at_the_saved_page() {
        let recent: Recent =
            serde_json::from_str(r#"{"path":"documents/manual.pdf","page":18}"#).unwrap();
        assert_eq!(recent.page, 18);
        assert_eq!(recent.offset, 0);
    }
}
