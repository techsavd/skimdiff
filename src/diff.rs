use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LineKind {
    Context,
    Add,
    Del,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, Serialize)]
pub struct Span {
    pub text: String,
    pub changed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Line {
    pub kind: LineKind,
    pub content: String,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spans: Option<Vec<Span>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Hunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub header: String,
    pub lines: Vec<Line>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileDiff {
    pub path: String,
    pub old_path: Option<String>,
    pub status: FileStatus,
    pub is_binary: bool,
    pub hunks: Vec<Hunk>,
}

/// Parse the output of `git diff --patch --no-color` into a structured model.
pub fn parse_patch(patch: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut old_no: u32 = 0;
    let mut new_no: u32 = 0;

    for raw in patch.lines() {
        if let Some(rest) = raw.strip_prefix("diff --git ") {
            let (old_p, new_p) = split_git_paths(rest);
            files.push(FileDiff {
                path: new_p.clone(),
                old_path: if old_p != new_p { Some(old_p) } else { None },
                status: FileStatus::Modified,
                is_binary: false,
                hunks: Vec::new(),
            });
            continue;
        }
        let Some(file) = files.last_mut() else { continue };

        if raw.starts_with("new file mode") {
            file.status = FileStatus::Added;
        } else if raw.starts_with("deleted file mode") {
            file.status = FileStatus::Deleted;
            // for deletions the meaningful path is the old one
            if let Some(old) = file.old_path.take() {
                file.path = old;
            }
        } else if let Some(from) = raw.strip_prefix("rename from ") {
            file.status = FileStatus::Renamed;
            file.old_path = Some(from.to_string());
        } else if raw.starts_with("rename to ") {
            file.status = FileStatus::Renamed;
        } else if raw.starts_with("Binary files ") {
            file.is_binary = true;
        } else if let Some(hdr) = raw.strip_prefix("@@ ") {
            if let Some(h) = parse_hunk_header(hdr) {
                old_no = h.old_start;
                new_no = h.new_start;
                file.hunks.push(h);
            }
        } else if let Some(hunk) = file.hunks.last_mut() {
            let (kind, content) = match raw.as_bytes().first() {
                Some(b' ') => (LineKind::Context, &raw[1..]),
                Some(b'+') => (LineKind::Add, &raw[1..]),
                Some(b'-') => (LineKind::Del, &raw[1..]),
                _ => continue, // e.g. "\ No newline at end of file"
            };
            let (o, n) = match kind {
                LineKind::Context => {
                    let r = (Some(old_no), Some(new_no));
                    old_no += 1;
                    new_no += 1;
                    r
                }
                LineKind::Del => {
                    let r = (Some(old_no), None);
                    old_no += 1;
                    r
                }
                LineKind::Add => {
                    let r = (None, Some(new_no));
                    new_no += 1;
                    r
                }
            };
            hunk.lines.push(Line {
                kind,
                content: content.to_string(),
                old_no: o,
                new_no: n,
                spans: None,
            });
        }
    }
    for f in &mut files {
        for h in &mut f.hunks {
            annotate_intraline(h);
        }
    }
    files
}

/// Pair each run of deleted lines with the run of added lines that follows it
/// and compute character-level changed/unchanged spans for each pair.
fn annotate_intraline(hunk: &mut Hunk) {
    let mut i = 0;
    while i < hunk.lines.len() {
        if hunk.lines[i].kind != LineKind::Del {
            i += 1;
            continue;
        }
        let del_start = i;
        while i < hunk.lines.len() && hunk.lines[i].kind == LineKind::Del {
            i += 1;
        }
        let add_start = i;
        while i < hunk.lines.len() && hunk.lines[i].kind == LineKind::Add {
            i += 1;
        }
        let pairs = (add_start - del_start).min(i - add_start);
        for k in 0..pairs {
            let (old, new) = (
                hunk.lines[del_start + k].content.clone(),
                hunk.lines[add_start + k].content.clone(),
            );
            let (o_spans, n_spans) = char_spans(&old, &new);
            hunk.lines[del_start + k].spans = Some(o_spans);
            hunk.lines[add_start + k].spans = Some(n_spans);
        }
    }
}

fn char_spans(old: &str, new: &str) -> (Vec<Span>, Vec<Span>) {
    use similar::ChangeTag;
    let diff = similar::TextDiff::from_chars(old, new);
    let mut o: Vec<Span> = Vec::new();
    let mut n: Vec<Span> = Vec::new();
    for ch in diff.iter_all_changes() {
        match ch.tag() {
            ChangeTag::Equal => {
                push_span(&mut o, ch.value(), false);
                push_span(&mut n, ch.value(), false);
            }
            ChangeTag::Delete => push_span(&mut o, ch.value(), true),
            ChangeTag::Insert => push_span(&mut n, ch.value(), true),
        }
    }
    (o, n)
}

fn push_span(v: &mut Vec<Span>, text: &str, changed: bool) {
    if let Some(last) = v.last_mut() {
        if last.changed == changed {
            last.text.push_str(text);
            return;
        }
    }
    v.push(Span {
        text: text.to_string(),
        changed,
    });
}

/// Build an all-added FileDiff for an untracked file.
pub fn synthetic_added(path: &str, content: &str) -> FileDiff {
    let lines: Vec<Line> = content
        .lines()
        .enumerate()
        .map(|(i, l)| Line {
            kind: LineKind::Add,
            content: l.to_string(),
            old_no: None,
            new_no: Some(i as u32 + 1),
            spans: None,
        })
        .collect();
    let n = lines.len() as u32;
    FileDiff {
        path: path.to_string(),
        old_path: None,
        status: FileStatus::Added,
        is_binary: false,
        hunks: if n == 0 {
            Vec::new()
        } else {
            vec![Hunk {
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: n,
                header: String::new(),
                lines,
            }]
        },
    }
}

/// "a/Old.java b/New.java" -> ("Old.java", "New.java")
fn split_git_paths(rest: &str) -> (String, String) {
    if let Some(idx) = rest.find(" b/") {
        let old = rest[..idx].strip_prefix("a/").unwrap_or(&rest[..idx]);
        let new = &rest[idx + 3..];
        (old.to_string(), new.to_string())
    } else {
        (rest.to_string(), rest.to_string())
    }
}

/// "-1,4 +1,4 @@ optional header"
fn parse_hunk_header(hdr: &str) -> Option<Hunk> {
    let end = hdr.find("@@")?;
    let ranges = hdr[..end].trim();
    let header = hdr[end + 2..].trim().to_string();
    let mut parts = ranges.split_whitespace();
    let (old_start, old_lines) = parse_range(parts.next()?.strip_prefix('-')?)?;
    let (new_start, new_lines) = parse_range(parts.next()?.strip_prefix('+')?)?;
    Some(Hunk {
        old_start,
        old_lines,
        new_start,
        new_lines,
        header,
        lines: Vec::new(),
    })
}

/// "1,4" -> (1,4); "1" -> (1,1)
fn parse_range(s: &str) -> Option<(u32, u32)> {
    match s.split_once(',') {
        Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
        None => Some((s.parse().ok()?, 1)),
    }
}
