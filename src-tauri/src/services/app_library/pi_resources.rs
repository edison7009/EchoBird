use super::{children, markdown_field, real_directory, Entry};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(super) fn instructions(directory: &Path) -> PathBuf {
    [
        "AGENTS.override.md",
        "AGENTS.md",
        "AGENTS.MD",
        "CLAUDE.md",
        "CLAUDE.MD",
    ]
    .into_iter()
    .map(|name| directory.join(name))
    .find(|path| path.is_file())
    .unwrap_or_else(|| directory.join("AGENTS.md"))
}

pub(super) fn skills(root: &Path, index: usize) -> Result<Vec<Entry>, String> {
    let mut entries = Vec::new();
    let mut directories = vec![(root.to_path_buf(), 0)];
    let writable_root = real_directory(root);
    while let Some((directory, depth)) = directories.pop() {
        let document = directory.join("SKILL.md");
        if document.is_file() {
            add_skill(
                &mut entries,
                root,
                index,
                directory,
                &document,
                writable_root && depth > 0,
            );
            continue;
        }
        for path in children(&directory)? {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if name.starts_with('.') || name == "node_modules" {
                continue;
            }
            if real_directory(&path) && depth < 31 {
                directories.push((path, depth + 1));
            } else if depth == 0
                && path.extension().is_some_and(|extension| extension == "md")
                && path.is_file()
            {
                add_skill(&mut entries, root, index, path.clone(), &path, false);
            } else if path.join("SKILL.md").is_file() {
                let document = path.join("SKILL.md");
                add_skill(&mut entries, root, index, path, &document, false);
            }
        }
    }
    Ok(entries)
}

fn add_skill(
    entries: &mut Vec<Entry>,
    root: &Path,
    index: usize,
    path: PathBuf,
    document: &Path,
    writable: bool,
) {
    let Some(description) = markdown_field(document, "description") else {
        return;
    };
    let fallback = if path == document {
        path.file_stem()
    } else {
        path.file_name()
    };
    let name = markdown_field(document, "name")
        .unwrap_or_else(|| fallback.unwrap_or_default().to_string_lossy().into_owned());
    let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy();
    let id = format!("{index}/{relative}");
    let regular = fs::symlink_metadata(document).is_ok_and(|metadata| metadata.is_file());
    let mut entry = Entry::new(name, Some(path));
    entry.id = id;
    entry.description = Some(description);
    entry.can_copy = writable && regular;
    entry.can_remove = entry.can_copy && index == 0;
    entries.push(entry);
}

pub(super) fn extensions(directory: &Path) -> Result<Vec<Entry>, String> {
    let mut entries: Vec<_> = children(&directory.join("extensions"))?
        .into_iter()
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?;
            if name.starts_with('.') {
                return None;
            }
            let manifest = || {
                fs::read(path.join("package.json"))
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            };
            let extension = path.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext == "ts" || ext == "js");
            let package = path.is_dir()
                && (path.join("index.ts").is_file()
                    || path.join("index.js").is_file()
                    || manifest().is_some_and(|value| {
                        value["pi"]["extensions"]
                            .as_array()
                            .is_some_and(|files| !files.is_empty())
                    }));
            (extension || package).then(|| Entry::new(name.into(), Some(path.clone())))
        })
        .collect();
    let settings = directory.join("settings.json");
    if let Some(bytes) =
        crate::services::native_account_files::read_optional(&settings).map_err(|_| "readFailed")?
    {
        let value: Value =
            serde_json::from_slice(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes))
                .map_err(|_| "invalidConfig")?;
        if let Some(packages) = value["packages"].as_array() {
            let mut seen = std::collections::HashSet::new();
            for package in packages {
                if let Some(source) = package.as_str().or_else(|| package["source"].as_str()) {
                    if seen.insert(source) {
                        let mut entry = Entry::new(source.into(), Some(settings.clone()));
                        entry.id = format!("package/{source}");
                        entries.push(entry);
                    }
                }
            }
        }
    }
    Ok(entries)
}
