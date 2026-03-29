use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// One directory completion candidate for the create-agent cwd field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DirectoryCompletionCandidate {
    pub(crate) display: String,
    pub(crate) completion_text: String,
}

/// Lists directory completion candidates for the path segment surrounding the cursor.
///
/// The returned completion text always contains the full replacement path with a trailing slash so
/// the caller can keep descending into nested directories.
pub(crate) fn complete_directory_segment(
    text: &str,
    cursor: usize,
) -> io::Result<Vec<DirectoryCompletionCandidate>> {
    complete_directory_segment_with(text, cursor, home_dir(), &std::env::current_dir()?)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn complete_directory_segment_with(
    text: &str,
    cursor: usize,
    home: Option<PathBuf>,
    current_dir: &Path,
) -> io::Result<Vec<DirectoryCompletionCandidate>> {
    let cursor = cursor.min(text.len());
    let cursor = previous_char_boundary(text, cursor).unwrap_or(cursor);
    let prefix = &text[..cursor];
    let Some(context) = completion_context(prefix, home.as_deref(), current_dir) else {
        return Ok(Vec::new());
    };

    let mut items = Vec::new();
    for entry in fs::read_dir(&context.expanded_parent)? {
        let entry = entry?;
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !context.segment_prefix.is_empty() && !name.starts_with(&context.segment_prefix) {
            continue;
        }
        if !context.segment_prefix.starts_with('.') && name.starts_with('.') {
            continue;
        }
        items.push(DirectoryCompletionCandidate {
            display: format!("{name}/"),
            completion_text: format!("{}{name}/", context.display_parent),
        });
    }
    items.sort_by(|left, right| left.display.cmp(&right.display));
    Ok(items)
}

struct CompletionContext {
    display_parent: String,
    expanded_parent: PathBuf,
    segment_prefix: String,
}

fn completion_context(
    prefix: &str,
    home: Option<&Path>,
    current_dir: &Path,
) -> Option<CompletionContext> {
    if prefix == "~" {
        let home = home?;
        return Some(CompletionContext {
            display_parent: "~/".to_owned(),
            expanded_parent: home.to_path_buf(),
            segment_prefix: String::new(),
        });
    }

    if let Some(remainder) = prefix.strip_prefix("~/") {
        let home = home?;
        let (parent_tail, segment_prefix) = split_parent_and_segment(remainder);
        return Some(CompletionContext {
            display_parent: format!("~/{}", parent_tail),
            expanded_parent: join_components(home, &parent_tail),
            segment_prefix,
        });
    }

    if let Some(remainder) = prefix.strip_prefix('/') {
        let (parent_tail, segment_prefix) = split_parent_and_segment(remainder);
        return Some(CompletionContext {
            display_parent: format!("/{parent_tail}"),
            expanded_parent: join_components(Path::new("/"), &parent_tail),
            segment_prefix,
        });
    }

    let (parent_tail, segment_prefix) = split_parent_and_segment(prefix);
    Some(CompletionContext {
        display_parent: parent_tail.clone(),
        expanded_parent: join_components(current_dir, &parent_tail),
        segment_prefix,
    })
}

fn split_parent_and_segment(text: &str) -> (String, String) {
    match text.rsplit_once('/') {
        Some((parent, segment)) => (format!("{parent}/"), segment.to_owned()),
        None => (String::new(), text.to_owned()),
    }
}

fn join_components(base: &Path, tail: &str) -> PathBuf {
    let mut path = base.to_path_buf();
    for component in tail.split('/').filter(|component| !component.is_empty()) {
        path.push(component);
    }
    path
}

fn previous_char_boundary(text: &str, index: usize) -> Option<usize> {
    if text.is_char_boundary(index) {
        return Some(index);
    }
    let mut candidate = index;
    while candidate > 0 {
        candidate -= 1;
        if text.is_char_boundary(candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        ffi::OsStr,
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("neozeus-{prefix}-{nanos}-{}", std::process::id()))
    }

    fn dir_name(path: &Path) -> String {
        path.file_name().and_then(OsStr::to_str).unwrap().to_owned()
    }

    /// Verifies that home-relative completion expands `~/` for lookup but preserves it in output.
    #[test]
    fn completes_home_relative_directories() {
        let home = unique_temp_dir("home-complete");
        fs::create_dir_all(home.join("code")).unwrap();
        fs::create_dir_all(home.join("configs")).unwrap();

        let items =
            complete_directory_segment_with("~/co", 4, Some(home.clone()), Path::new("/tmp"))
                .unwrap();

        assert_eq!(
            items,
            vec![
                DirectoryCompletionCandidate {
                    display: "code/".into(),
                    completion_text: "~/code/".into(),
                },
                DirectoryCompletionCandidate {
                    display: "configs/".into(),
                    completion_text: "~/configs/".into(),
                },
            ]
        );

        let _ = fs::remove_dir_all(home);
    }

    /// Verifies that completion filters out non-directories and hidden entries unless requested.
    #[test]
    fn completion_filters_non_directories_and_hidden_entries() {
        let root = unique_temp_dir("filter-complete");
        fs::create_dir_all(root.join("alpha")).unwrap();
        fs::create_dir_all(root.join(".secret")).unwrap();
        fs::write(root.join("alpha.txt"), "x").unwrap();

        let visible = complete_directory_segment_with("a", 1, None, &root).unwrap();
        assert_eq!(
            visible,
            vec![DirectoryCompletionCandidate {
                display: "alpha/".into(),
                completion_text: "alpha/".into(),
            }]
        );

        let hidden = complete_directory_segment_with(".", 1, None, &root).unwrap();
        assert_eq!(
            hidden,
            vec![DirectoryCompletionCandidate {
                display: ".secret/".into(),
                completion_text: ".secret/".into(),
            }]
        );

        let _ = fs::remove_dir_all(root);
    }

    /// Verifies that absolute-path completion replaces only the final segment and sorts results.
    #[test]
    fn completion_sorts_absolute_path_matches() {
        let root = unique_temp_dir("absolute-complete");
        let zoo = root.join("zoo");
        fs::create_dir_all(zoo.join("beta")).unwrap();
        fs::create_dir_all(zoo.join("alpha")).unwrap();

        let prefix = format!("{}/a", zoo.display());
        let items = complete_directory_segment_with(&prefix, prefix.len(), None, Path::new("/tmp"))
            .unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].display, "alpha/");
        assert_eq!(
            items[0].completion_text,
            format!("{}/alpha/", zoo.display())
        );
        assert_eq!(dir_name(&PathBuf::from(&items[0].completion_text)), "alpha");

        let _ = fs::remove_dir_all(root);
    }
}
