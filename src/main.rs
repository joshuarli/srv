use std::{
    borrow::Cow,
    cmp::Ordering,
    env,
    fmt::Write as _,
    fs::{self, File},
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::io::AsRawFd,
    path::{Path, PathBuf},
};

use pulldown_cmark::{Options, Parser, html};

const LISTING_PRELUDE: &str = "<head><link rel=icon href=data:,><style>* { font-family: monospace; } \
     table { border: none; margin: 1rem; } td { padding-right: 2rem; }</style></head>\n\
     <table>";

const MARKDOWN_PRELUDE: &str = "<!doctype html><html><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<title>Markdown</title><link rel=icon href=data:,><style>\
:root { color-scheme: light; }\
body { margin: 0; background: #f6f8fa; color: #24292f; }\
.markdown-body { box-sizing: border-box; max-width: 980px; margin: 0 auto; padding: 40px; \
font-family: -apple-system, BlinkMacSystemFont, \"Segoe UI\", Helvetica, Arial, sans-serif; \
line-height: 1.6; }\
@media (max-width: 767px) { .markdown-body { padding: 20px; } }\
.markdown-body h1, .markdown-body h2 { border-bottom: 1px solid #d0d7de; padding-bottom: .3em; }\
.markdown-body code, .markdown-body pre { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }\
.markdown-body pre { background: #f6f8fa; padding: 16px; border-radius: 6px; overflow: auto; }\
.markdown-body code { background: rgba(175,184,193,.2); padding: .2em .4em; border-radius: 6px; }\
.markdown-body pre code { background: transparent; padding: 0; }\
.markdown-body table { border-collapse: collapse; }\
.markdown-body th, .markdown-body td { border: 1px solid #d0d7de; padding: 6px 13px; }\
.markdown-body tr:nth-child(2n) { background: #f6f8fa; }\
.markdown-body blockquote { margin: 0; padding: 0 1em; color: #57606a; border-left: .25em solid #d0d7de; }\
.markdown-body a { color: #0969da; text-decoration: none; }\
.markdown-body a:hover { text-decoration: underline; }\
</style></head><body><article class=\"markdown-body\">";

const MARKDOWN_POSTLUDE: &str = "</article></body></html>";

const DOCS_PAGE: &str = r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Docs</title>
<link rel="icon" href="data:,">
<style>
* { box-sizing: border-box; }
body { margin: 0; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif; color: #24292f; background: #fff; }
#app { display: grid; grid-template-columns: 300px 1fr; height: 100vh; }
#tree { overflow: auto; border-right: 1px solid #ececf1; background: #f7f7f8; padding: 10px 8px 14px; }
#viewer { overflow: auto; padding: 24px; }
#status { color: #8e8ea0; font-size: 11px; letter-spacing: .08em; text-transform: uppercase; padding: 8px 10px; }
#tree-root { display: flex; flex-direction: column; gap: 2px; }
details { margin: 0; border-radius: 8px; }
summary { cursor: pointer; user-select: none; list-style: none; font-size: 13px; color: #353740; padding: 6px 10px; border-radius: 8px; display: flex; align-items: center; gap: 6px; }
summary::-webkit-details-marker { display: none; }
summary::before { content: "▸"; font-size: 10px; color: #8e8ea0; transform-origin: 45% 50%; transition: transform .12s ease; }
details[open] > summary::before { transform: rotate(90deg); }
summary:hover { background: #ececf1; }
.dir-children { margin-left: 14px; padding-left: 8px; border-left: 1px solid #e3e3e8; display: flex; flex-direction: column; gap: 2px; }
button.file { display: block; width: 100%; text-align: left; border: 0; background: transparent; padding: 6px 10px; border-radius: 8px; cursor: pointer; font: inherit; font-size: 13px; color: #2f3138; }
button.file:hover { background: #ececf1; }
button.file.active { background: #e3e3e8; font-weight: 600; }
.markdown-body { max-width: 980px; margin: 0 auto; line-height: 1.6; }
.markdown-body h1, .markdown-body h2 { border-bottom: 1px solid #d0d7de; padding-bottom: .3em; }
.markdown-body code, .markdown-body pre { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
.markdown-body pre { background: #f6f8fa; padding: 16px; border-radius: 6px; overflow: auto; }
.markdown-body code { background: rgba(175,184,193,.2); padding: .2em .4em; border-radius: 6px; }
.markdown-body pre code { background: transparent; padding: 0; }
.markdown-body table { border-collapse: collapse; }
.markdown-body th, .markdown-body td { border: 1px solid #d0d7de; padding: 6px 13px; }
.markdown-body tr:nth-child(2n) { background: #f6f8fa; }
@media (max-width: 900px) {
  #app { grid-template-columns: 1fr; }
  #tree { height: 40vh; border-right: 0; border-bottom: 1px solid #d0d7de; }
}
</style>
</head>
<body>
<div id="app">
  <aside id="tree">
    <div id="status">Loading files...</div>
    <div id="tree-root"></div>
  </aside>
  <main id="viewer">
    <article class="markdown-body">
      <p>Select a Markdown file from the left.</p>
    </article>
  </main>
</div>
<script>
const statusEl = document.getElementById("status");
const treeRootEl = document.getElementById("tree-root");
const viewerEl = document.getElementById("viewer");
let activeBtn = null;

function create(tag, attrs = {}) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "text") {
      el.textContent = v;
    } else {
      el.setAttribute(k, v);
    }
  }
  return el;
}

async function loadMarkdown(path, btn) {
  if (activeBtn) activeBtn.classList.remove("active");
  activeBtn = btn || null;
  if (activeBtn) activeBtn.classList.add("active");
  statusEl.textContent = path;
  const resp = await fetch("/__docs__/md?path=" + encodeURIComponent(path));
  if (!resp.ok) {
    viewerEl.innerHTML = '<article class="markdown-body"><p>Unable to load file.</p></article>';
    return;
  }
  const html = await resp.text();
  viewerEl.innerHTML = '<article class="markdown-body">' + html + '</article>';
}

function renderNode(node, parent) {
  if (node.dir) {
    const details = create("details");
    const summary = create("summary", { text: node.name });
    details.appendChild(summary);
    const container = create("div", { class: "dir-children" });
    details.appendChild(container);
    parent.appendChild(details);
    for (const child of node.children || []) renderNode(child, container);
    return;
  }

  const btn = create("button", { class: "file", text: node.name, type: "button" });
  btn.addEventListener("click", () => loadMarkdown(node.path, btn));
  parent.appendChild(btn);
}

async function init() {
  const resp = await fetch("/__docs__/tree");
  if (!resp.ok) {
    statusEl.textContent = "Failed to load file tree.";
    return;
  }
  const root = await resp.json();
  statusEl.textContent = "Ready";
  treeRootEl.innerHTML = "";
  for (const child of root.children || []) renderNode(child, treeRootEl);
}

init();
</script>
</body>
</html>
"#;

#[derive(Clone, Debug)]
struct DocsNode {
    name: String,
    path: String,
    dir: bool,
    children: Vec<DocsNode>,
}

#[derive(Clone, Debug)]
struct IgnorePattern {
    pattern: String,
    anchored: bool,
}

#[derive(Clone, Debug, Default)]
struct IgnoreRules {
    patterns: Vec<IgnorePattern>,
}

// --- humanize -----------------------------------------------------------------

fn file_size(n: u64) -> String {
    if n < 1024 {
        return format!("{n}");
    }
    let mut f = n as f64;
    let mut exp = 0usize;
    loop {
        f /= 1024.0;
        if f < 1024.0 || exp >= 3 {
            break;
        }
        exp += 1;
    }
    format!("{f:.1}{}", b"KMGT"[exp] as char)
}

// Ported from github.com/fvbommel/util sortorder/natsort.go (via Go version)
fn natural_cmp(s1: &[u8], s2: &[u8]) -> Ordering {
    let (mut i1, mut i2) = (0, 0);
    while i1 < s1.len() && i2 < s2.len() {
        let (c1, c2) = (s1[i1], s2[i2]);
        let (d1, d2) = (c1.is_ascii_digit(), c2.is_ascii_digit());
        match (d1, d2) {
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            (false, false) => match c1.cmp(&c2) {
                Ordering::Equal => {
                    i1 += 1;
                    i2 += 1;
                }
                ord => return ord,
            },
            (true, true) => {
                while i1 < s1.len() && s1[i1] == b'0' {
                    i1 += 1;
                }
                while i2 < s2.len() && s2[i2] == b'0' {
                    i2 += 1;
                }
                let (nz1, nz2) = (i1, i2);
                while i1 < s1.len() && s1[i1].is_ascii_digit() {
                    i1 += 1;
                }
                while i2 < s2.len() && s2[i2].is_ascii_digit() {
                    i2 += 1;
                }
                match (i1 - nz1).cmp(&(i2 - nz2)) {
                    Ordering::Equal => {}
                    ord => return ord,
                }
                match s1[nz1..i1].cmp(&s2[nz2..i2]) {
                    Ordering::Equal => {}
                    ord => return ord,
                }
                match nz1.cmp(&nz2) {
                    Ordering::Equal => {}
                    ord => return ord,
                }
            }
        }
    }
    s1.len().cmp(&s2.len())
}

// --- URL encoding -------------------------------------------------------------

fn percent_decode(s: &str) -> Cow<'_, str> {
    if !s.as_bytes().contains(&b'%') {
        return Cow::Borrowed(s);
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex_val(b[i + 1]), hex_val(b[i + 2]))
        {
            out.push((h << 4) | l);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    Cow::Owned(
        String::from_utf8(out)
            .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()),
    )
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &byte in s.as_bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b'!'
            | b'\''
            | b'('
            | b')'
            | b'*' => out.push(byte as char),
            b => write!(out, "%{b:02X}").unwrap(),
        }
    }
    out
}

// --- Path helpers -------------------------------------------------------------

/// Normalize a request path into `out`, reusing its allocation across calls.
/// Returns `true` if the result is within `base`, `false` for traversal.
fn normalize_into(base: &Path, raw: &str, out: &mut PathBuf) -> bool {
    out.as_mut_os_string().clear();
    out.push(base);
    let raw = raw.split('?').next().unwrap_or("/");
    for seg in raw.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    out.starts_with(base)
}

// --- MIME types ---------------------------------------------------------------

fn mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css",
        "js" | "mjs" => "application/javascript",
        "json" => "application/json",
        "xml" => "application/xml",
        "txt" | "md" => "text/plain; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "webp" => "image/webp",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "pdf" => "application/pdf",
        "wasm" => "application/wasm",
        "zip" => "application/zip",
        "gz" | "tgz" => "application/gzip",
        _ => "application/octet-stream",
    }
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"))
}

fn gfm_options() -> Options {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts
}

fn render_markdown_fragment(markdown: &str, html_out: &mut String) {
    html_out.clear();
    let parser = Parser::new_ext(markdown, gfm_options());
    html::push_html(html_out, parser);
}

fn render_markdown(markdown: &str, html_out: &mut String) {
    html_out.clear();
    html_out.push_str(MARKDOWN_PRELUDE);
    let parser = Parser::new_ext(markdown, gfm_options());
    html::push_html(html_out, parser);
    html_out.push_str(MARKDOWN_POSTLUDE);
}

fn is_hidden_dir_name(name: &str) -> bool {
    name.starts_with('.')
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let p = pattern.as_bytes();
    let t = text.as_bytes();
    let (mut i, mut j) = (0usize, 0usize);
    let (mut star, mut match_j) = (None::<usize>, 0usize);

    while j < t.len() {
        if i < p.len() && (p[i] == b'?' || p[i] == t[j]) {
            i += 1;
            j += 1;
        } else if i < p.len() && p[i] == b'*' {
            star = Some(i);
            i += 1;
            match_j = j;
        } else if let Some(star_i) = star {
            i = star_i + 1;
            match_j += 1;
            j = match_j;
        } else {
            return false;
        }
    }

    while i < p.len() && p[i] == b'*' {
        i += 1;
    }
    i == p.len()
}

fn read_ignore_rules(root: &Path) -> IgnoreRules {
    let path = root.join(".gitignore");
    let mut rules = IgnoreRules::default();
    let Ok(raw) = fs::read_to_string(path) else {
        return rules;
    };

    for line in raw.lines() {
        let mut line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
            continue;
        }
        if let Some(stripped) = line.strip_prefix("./") {
            line = stripped;
        }

        let anchored = line.starts_with('/');
        if anchored {
            line = line.trim_start_matches('/');
        }

        line = line.trim_end_matches('/');
        if line.is_empty() {
            continue;
        }

        rules.patterns.push(IgnorePattern {
            pattern: line.to_string(),
            anchored,
        });
    }

    rules
}

fn is_gitignored_dir(rel: &str, name: &str, rules: &IgnoreRules) -> bool {
    for rule in &rules.patterns {
        let pat = rule.pattern.as_str();
        if rule.anchored {
            if glob_match(pat, rel) || rel.starts_with(&format!("{pat}/")) {
                return true;
            }
            continue;
        }

        if pat.contains('/') {
            if glob_match(pat, rel) || rel.ends_with(&format!("/{pat}")) {
                return true;
            }
            continue;
        }

        if glob_match(pat, name) {
            return true;
        }
    }
    false
}

fn json_string(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => write!(out, "\\u{:04x}", c as u32).unwrap(),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn node_to_json(node: &DocsNode, out: &mut String) {
    out.push('{');
    out.push_str("\"name\":");
    json_string(&node.name, out);
    out.push(',');
    out.push_str("\"path\":");
    json_string(&node.path, out);
    out.push(',');
    out.push_str("\"dir\":");
    out.push_str(if node.dir { "true" } else { "false" });
    if node.dir {
        out.push(',');
        out.push_str("\"children\":[");
        for (idx, child) in node.children.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            node_to_json(child, out);
        }
        out.push(']');
    }
    out.push('}');
}

fn rel_to_web_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn walk_docs_dir(
    root: &Path,
    rel: &Path,
    ignore: &IgnoreRules,
    parallel: bool,
) -> io::Result<Option<DocsNode>> {
    let dir_path = if rel.as_os_str().is_empty() {
        root.to_path_buf()
    } else {
        root.join(rel)
    };

    let mut entries: Vec<_> = fs::read_dir(&dir_path)?.filter_map(|e| e.ok()).collect();
    entries.sort_by(|a, b| {
        let an = a.file_name().to_string_lossy().to_lowercase();
        let bn = b.file_name().to_string_lossy().to_lowercase();
        natural_cmp(an.as_bytes(), bn.as_bytes())
    });

    let mut file_nodes = Vec::new();
    let mut subdirs = Vec::new();

    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let ft = meta.file_type();
        if ft.is_dir() {
            if is_hidden_dir_name(&name) {
                continue;
            }
            let child_rel = rel.join(&name);
            let rel_str = rel_to_web_path(&child_rel);
            if is_gitignored_dir(&rel_str, &name, ignore) {
                continue;
            }
            subdirs.push((name, child_rel));
        } else if ft.is_file() {
            let fp = entry.path();
            if is_markdown(&fp) {
                let rel_path = rel_to_web_path(&rel.join(&name));
                file_nodes.push(DocsNode {
                    name,
                    path: rel_path,
                    dir: false,
                    children: Vec::new(),
                });
            }
        }
    }

    let mut dir_nodes = Vec::new();
    if parallel && !subdirs.is_empty() {
        std::thread::scope(|scope| {
            let mut jobs = Vec::new();
            for (_, child_rel) in &subdirs {
                jobs.push(scope.spawn(move || walk_docs_dir(root, child_rel, ignore, false)));
            }

            for job in jobs {
                let joined = match job.join() {
                    Ok(v) => v,
                    Err(_) => return Err(io::Error::other("docs tree worker thread panicked")),
                }?;
                if let Some(node) = joined {
                    dir_nodes.push(node);
                }
            }
            Ok::<(), io::Error>(())
        })?;
    } else {
        for (_, child_rel) in &subdirs {
            if let Some(node) = walk_docs_dir(root, child_rel, ignore, false)? {
                dir_nodes.push(node);
            }
        }
    }

    let name = if rel.as_os_str().is_empty() {
        "/".to_string()
    } else {
        rel.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string()
    };

    let mut children = Vec::with_capacity(dir_nodes.len() + file_nodes.len());
    children.extend(dir_nodes);
    children.extend(file_nodes);

    if !rel.as_os_str().is_empty() && children.is_empty() {
        return Ok(None);
    }

    Ok(Some(DocsNode {
        name,
        path: rel_to_web_path(rel),
        dir: true,
        children,
    }))
}

fn render_docs_tree_json(root: &Path, out: &mut String) -> io::Result<()> {
    let ignore = read_ignore_rules(root);
    let root_node = walk_docs_dir(root, Path::new(""), &ignore, true)?.unwrap_or(DocsNode {
        name: "/".to_string(),
        path: String::new(),
        dir: true,
        children: Vec::new(),
    });
    out.clear();
    node_to_json(&root_node, out);
    Ok(())
}

fn query_param(path_raw: &str, key: &str) -> Option<String> {
    let query = path_raw.split_once('?')?.1;
    for part in query.split('&') {
        let (k, v) = part.split_once('=').unwrap_or((part, ""));
        if k == key {
            return Some(percent_decode(v).into_owned());
        }
    }
    None
}

// --- Zero-copy file transfer --------------------------------------------------

#[cfg(target_os = "macos")]
fn send_file(file: &File, sock: &TcpStream, len: u64) -> io::Result<()> {
    unsafe extern "C" {
        fn sendfile(fd: i32, s: i32, offset: i64, len: *mut i64, hdtr: *mut (), flags: i32) -> i32;
    }
    let mut remaining = len as i64;
    let mut offset: i64 = 0;
    while remaining > 0 {
        let mut chunk = remaining;
        let ret = unsafe {
            sendfile(
                file.as_raw_fd(),
                sock.as_raw_fd(),
                offset,
                &mut chunk,
                std::ptr::null_mut(),
                0,
            )
        };
        if chunk > 0 {
            offset += chunk;
            remaining -= chunk;
        }
        if ret == -1 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn send_file(file: &File, sock: &TcpStream, len: u64) -> io::Result<()> {
    unsafe extern "C" {
        fn sendfile(out_fd: i32, in_fd: i32, offset: *mut i64, count: usize) -> isize;
    }
    let mut offset: i64 = 0;
    let mut remaining = len as usize;
    while remaining > 0 {
        let n = unsafe { sendfile(sock.as_raw_fd(), file.as_raw_fd(), &mut offset, remaining) };
        if n == -1 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
        remaining -= n as usize;
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn send_file(file: &File, sock: &TcpStream, _len: u64) -> io::Result<()> {
    io::copy(&mut file.try_clone()?, &mut sock.try_clone()?)?;
    Ok(())
}

// --- HTTP helpers (all stack-buffered) ----------------------------------------

fn write_headers(
    mut sock: &TcpStream,
    status: u16,
    reason: &str,
    ct: &str,
    cl: u64,
) -> io::Result<()> {
    let mut hdr = [0u8; 512];
    let n = {
        let mut c = io::Cursor::new(&mut hdr[..]);
        write!(
            c,
            "HTTP/1.1 {status} {reason}\r\n\
             Content-Type: {ct}\r\n\
             Content-Length: {cl}\r\n\
             Cache-Control: no-store\r\n\
             Connection: close\r\n\
             \r\n"
        )?;
        c.position() as usize
    };
    sock.write_all(&hdr[..n])
}

fn write_error(mut sock: &TcpStream, code: u16, msg: &str) -> io::Result<()> {
    let mut buf = [0u8; 512];
    let body_len = 4 + msg.len();
    let n = {
        let mut c = io::Cursor::new(&mut buf[..]);
        write!(
            c,
            "HTTP/1.1 {code} {msg}\r\n\
             Content-Type: text/plain\r\n\
             Content-Length: {body_len}\r\n\
             Cache-Control: no-store\r\n\
             Connection: close\r\n\
             \r\n\
             {code} {msg}"
        )?;
        c.position() as usize
    };
    sock.write_all(&buf[..n])
}

fn write_body(sock: &TcpStream, ct: &str, body: &[u8]) -> io::Result<()> {
    write_headers(sock, 200, "OK", ct, body.len() as u64)?;
    let mut w: &TcpStream = sock;
    w.write_all(body)
}

fn serve_file(sock: &TcpStream, path: &Path, len: u64, content_type: &str) -> io::Result<()> {
    let file = File::open(path)?;
    write_headers(sock, 200, "OK", content_type, len)?;
    send_file(&file, sock, len)
}

fn serve_markdown(sock: &TcpStream, path: &Path, html_out: &mut String) -> io::Result<()> {
    let markdown = fs::read(path)?;
    let markdown = String::from_utf8_lossy(&markdown);
    render_markdown(&markdown, html_out);
    write_headers(
        sock,
        200,
        "OK",
        "text/html; charset=utf-8",
        html_out.len() as u64,
    )?;
    let mut w: &TcpStream = sock;
    w.write_all(html_out.as_bytes())
}

fn serve_markdown_fragment(sock: &TcpStream, path: &Path, html_out: &mut String) -> io::Result<()> {
    let markdown = fs::read(path)?;
    let markdown = String::from_utf8_lossy(&markdown);
    render_markdown_fragment(&markdown, html_out);
    write_body(sock, "text/html; charset=utf-8", html_out.as_bytes())
}

// --- Directory listing --------------------------------------------------------

fn render_listing(dir: &Path, html: &mut String) -> io::Result<()> {
    html.clear();
    html.push_str(LISTING_PRELUDE);

    // Precompute lowercase sort keys (Schwartzian transform)
    let mut entries: Vec<_> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| {
            let key = e.file_name().to_string_lossy().to_lowercase();
            (key, e)
        })
        .collect();
    entries.sort_by(|(a, _), (b, _)| natural_cmp(a.as_bytes(), b.as_bytes()));

    for (_, entry) in &entries {
        let name_os = entry.file_name();
        let name = name_os.to_string_lossy();
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let ft = meta.file_type();
        if ft.is_dir() {
            let enc = percent_encode(&name);
            write!(html, "<tr><td><a href=\"{enc}/\">{name}/</a></td></tr>").unwrap();
        } else if ft.is_file() {
            let enc = percent_encode(&name);
            let sz = file_size(meta.len());
            write!(
                html,
                "<tr><td><a href=\"{enc}\">{name}</a></td><td>{sz}</td></tr>"
            )
            .unwrap();
        } else {
            write!(
                html,
                "<tr><td><p style=\"color: #777\">{name}</p></td></tr>"
            )
            .unwrap();
        }
    }
    html.push_str("</table>");
    Ok(())
}

// --- Server ------------------------------------------------------------------

struct Server {
    dir: PathBuf,
    quiet: bool,
    docs_mode: bool,
    fp: PathBuf,
    html: String,
}

impl Server {
    fn handle(&mut self, mut stream: TcpStream) -> io::Result<()> {
        let mut buf = [0u8; 8192];
        let mut pos = 0usize;

        loop {
            if pos == buf.len() {
                return write_error(&stream, 431, "Request Header Fields Too Large");
            }
            let n = stream.read(&mut buf[pos..])?;
            if n == 0 {
                return Ok(());
            }
            pos += n;
            let mut hdrs = [httparse::EMPTY_HEADER; 16];
            let mut req = httparse::Request::new(&mut hdrs);
            match req.parse(&buf[..pos]) {
                Ok(httparse::Status::Complete(_)) => {
                    return self.dispatch(
                        &stream,
                        req.method.unwrap_or(""),
                        req.path.unwrap_or("/"),
                    );
                }
                Ok(httparse::Status::Partial) => continue,
                Err(_) => return write_error(&stream, 400, "Bad Request"),
            }
        }
    }

    fn dispatch(&mut self, sock: &TcpStream, method: &str, path_raw: &str) -> io::Result<()> {
        if !self.quiet
            && let Ok(peer) = sock.peer_addr()
        {
            eprintln!("\t{peer}: {method} {path_raw}");
        }

        if method != "GET" {
            return write_error(sock, 405, "Method Not Allowed");
        }

        if self.docs_mode {
            return self.dispatch_docs(sock, path_raw);
        }

        let decoded = percent_decode(path_raw);
        if !normalize_into(&self.dir, &decoded, &mut self.fp) {
            return write_error(sock, 403, "Forbidden");
        }

        let meta = match fs::symlink_metadata(self.fp.as_path()) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return write_error(sock, 404, "Not Found");
            }
            Err(e) => return Err(e),
        };

        let ft = meta.file_type();
        if ft.is_dir() {
            self.fp.push("index.html");
            if let Ok(idx_meta) = fs::metadata(self.fp.as_path())
                && idx_meta.is_file()
            {
                let result = serve_file(sock, &self.fp, idx_meta.len(), "text/html; charset=utf-8");
                self.fp.pop();
                return result;
            }
            self.fp.pop();

            render_listing(&self.fp, &mut self.html)?;
            write_headers(
                sock,
                200,
                "OK",
                "text/html; charset=utf-8",
                self.html.len() as u64,
            )?;
            let mut w: &TcpStream = sock;
            w.write_all(self.html.as_bytes())?;
        } else if ft.is_file() {
            if is_markdown(&self.fp) {
                serve_markdown(sock, &self.fp, &mut self.html)?;
            } else {
                serve_file(sock, &self.fp, meta.len(), mime(&self.fp))?;
            }
        } else if ft.is_symlink() {
            write_error(sock, 403, "Forbidden: symlinks not served")?;
        } else {
            write_error(sock, 403, "Forbidden: not a regular file or directory")?;
        }

        Ok(())
    }

    fn dispatch_docs(&mut self, sock: &TcpStream, path_raw: &str) -> io::Result<()> {
        let path = path_raw.split('?').next().unwrap_or("/");
        match path {
            "/__docs__/tree" => self.serve_docs_tree(sock),
            "/__docs__/md" => self.serve_docs_markdown(sock, path_raw),
            _ => write_body(sock, "text/html; charset=utf-8", DOCS_PAGE.as_bytes()),
        }
    }

    fn serve_docs_tree(&mut self, sock: &TcpStream) -> io::Result<()> {
        render_docs_tree_json(&self.dir, &mut self.html)?;
        write_body(sock, "application/json; charset=utf-8", self.html.as_bytes())
    }

    fn serve_docs_markdown(&mut self, sock: &TcpStream, path_raw: &str) -> io::Result<()> {
        let Some(rel) = query_param(path_raw, "path") else {
            return write_error(sock, 400, "Bad Request: missing path parameter");
        };

        let rel = rel.trim_start_matches('/');
        if rel.is_empty() {
            return write_error(sock, 400, "Bad Request: empty path parameter");
        }
        let req_path = format!("/{rel}");
        if !normalize_into(&self.dir, &req_path, &mut self.fp) {
            return write_error(sock, 403, "Forbidden");
        }
        if !is_markdown(&self.fp) {
            return write_error(sock, 400, "Bad Request: path must be a markdown file");
        }
        let meta = match fs::metadata(&self.fp) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return write_error(sock, 404, "Not Found"),
            Err(e) => return Err(e),
        };
        if !meta.is_file() {
            return write_error(sock, 404, "Not Found");
        }

        serve_markdown_fragment(sock, &self.fp, &mut self.html)
    }
}

// --- main --------------------------------------------------------------------

fn die(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(1);
}

fn main() {
    let mut quiet = false;
    let mut docs_mode = false;
    let mut port = String::from("8000");
    let mut bind = String::from("127.0.0.1");
    let mut dir = String::from(".");

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-q" => quiet = true,
            "--docs" => docs_mode = true,
            "-p" => port = args.next().unwrap_or_else(|| die("-p requires a value")),
            "-b" => bind = args.next().unwrap_or_else(|| die("-b requires a value")),
            "-V" | "--version" => {
                println!("srv {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "-h" | "--help" => {
                eprintln!(
                    "usage: srv [-q] [--docs] [-p port] [-b address] [directory]\n\
                     \n\
                     directory    path to serve (default: .)\n\
                     -q           quiet; disable logging\n\
                     --docs       launch docs browser for markdown files\n\
                     -p port      port to listen on (default: 8000)\n\
                     -b address   bind address (default: 127.0.0.1)\n\
                     -V           print version and exit"
                );
                std::process::exit(0);
            }
            _ => dir = arg,
        }
    }

    let dir = match fs::canonicalize(&dir) {
        Ok(p) => p,
        Err(e) => die(&format!("{dir}: {e}")),
    };
    if !dir.is_dir() {
        die(&format!("{} is not a directory", dir.display()));
    }

    let addr = format!("{bind}:{port}");
    let listener =
        TcpListener::bind(&addr).unwrap_or_else(|e| die(&format!("failed to bind {addr}: {e}")));

    eprintln!("\tServing {} over HTTP on {addr}", dir.display());

    let mut srv = Server {
        dir,
        quiet,
        docs_mode,
        fp: PathBuf::new(),
        html: String::new(),
    };

    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                if let Err(e) = srv.handle(s) {
                    eprintln!("error: {e}");
                }
            }
            Err(e) => eprintln!("accept error: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;

    // --- file_size ---------------------------------------------------------------

    #[test]
    fn file_size_bytes() {
        assert_eq!(file_size(0), "0");
        assert_eq!(file_size(1), "1");
        assert_eq!(file_size(1023), "1023");
    }

    #[test]
    fn file_size_kilo() {
        assert_eq!(file_size(1024), "1.0K");
        assert_eq!(file_size(1536), "1.5K");
        assert_eq!(file_size(1024 * 1023), "1023.0K");
    }

    #[test]
    fn file_size_mega() {
        assert_eq!(file_size(1024 * 1024), "1.0M");
        assert_eq!(file_size(1024 * 1024 * 5 + 1024 * 512), "5.5M");
    }

    #[test]
    fn file_size_giga() {
        assert_eq!(file_size(1024 * 1024 * 1024), "1.0G");
    }

    #[test]
    fn file_size_tera() {
        assert_eq!(file_size(1024u64 * 1024 * 1024 * 1024), "1.0T");
    }

    // --- natural_cmp -------------------------------------------------------------

    #[test]
    fn natural_cmp_basic() {
        assert_eq!(natural_cmp(b"a", b"b"), Ordering::Less);
        assert_eq!(natural_cmp(b"b", b"a"), Ordering::Greater);
        assert_eq!(natural_cmp(b"a", b"a"), Ordering::Equal);
    }

    #[test]
    fn natural_cmp_numeric() {
        assert_eq!(natural_cmp(b"file2", b"file10"), Ordering::Less);
        assert_eq!(natural_cmp(b"file10", b"file2"), Ordering::Greater);
        assert_eq!(natural_cmp(b"file1", b"file2"), Ordering::Less);
    }

    #[test]
    fn natural_cmp_leading_zeros() {
        assert_eq!(natural_cmp(b"file01", b"file001"), Ordering::Less);
    }

    #[test]
    fn natural_cmp_digits_before_letters() {
        assert_eq!(natural_cmp(b"1abc", b"abc"), Ordering::Less);
        assert_eq!(natural_cmp(b"abc", b"1abc"), Ordering::Greater);
    }

    #[test]
    fn natural_cmp_prefix() {
        assert_eq!(natural_cmp(b"file", b"file1"), Ordering::Less);
        assert_eq!(natural_cmp(b"file1", b"file"), Ordering::Greater);
    }

    #[test]
    fn natural_cmp_empty() {
        assert_eq!(natural_cmp(b"", b"a"), Ordering::Less);
        assert_eq!(natural_cmp(b"a", b""), Ordering::Greater);
        assert_eq!(natural_cmp(b"", b""), Ordering::Equal);
    }

    // --- percent_decode / percent_encode -----------------------------------------

    #[test]
    fn percent_decode_passthrough() {
        assert_eq!(percent_decode("hello").as_ref(), "hello");
    }

    #[test]
    fn percent_decode_passthrough_borrows() {
        assert!(matches!(percent_decode("hello"), Cow::Borrowed(_)));
    }

    #[test]
    fn percent_decode_space() {
        assert_eq!(percent_decode("hello%20world").as_ref(), "hello world");
    }

    #[test]
    fn percent_decode_slash() {
        assert_eq!(percent_decode("%2Fetc%2Fpasswd").as_ref(), "/etc/passwd");
    }

    #[test]
    fn percent_decode_invalid_seq() {
        assert_eq!(percent_decode("100%").as_ref(), "100%");
        assert_eq!(percent_decode("%ZZ").as_ref(), "%ZZ");
        assert_eq!(percent_decode("%0").as_ref(), "%0");
    }

    #[test]
    fn percent_encode_unreserved() {
        assert_eq!(percent_encode("hello"), "hello");
        assert_eq!(percent_encode("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn percent_encode_special() {
        assert_eq!(percent_encode("hello world"), "hello%20world");
        assert_eq!(percent_encode("a/b"), "a%2Fb");
        assert_eq!(percent_encode("a@b"), "a%40b");
    }

    #[test]
    fn percent_roundtrip() {
        let input = "file with spaces & (parens)";
        assert_eq!(percent_decode(&percent_encode(input)).as_ref(), input);
    }

    // --- normalize_into ----------------------------------------------------------

    fn normalize_test(base: &str, raw: &str) -> Option<PathBuf> {
        let base = Path::new(base);
        let mut fp = PathBuf::new();
        if normalize_into(base, raw, &mut fp) {
            Some(fp)
        } else {
            None
        }
    }

    #[test]
    fn normalize_root() {
        assert_eq!(normalize_test("/srv", "/"), Some(PathBuf::from("/srv")));
    }

    #[test]
    fn normalize_subpath() {
        assert_eq!(
            normalize_test("/srv", "/foo/bar"),
            Some(PathBuf::from("/srv/foo/bar"))
        );
    }

    #[test]
    fn normalize_traversal_basic() {
        assert_eq!(normalize_test("/srv/public", "/../../../etc/passwd"), None);
    }

    #[test]
    fn normalize_traversal_partial() {
        assert_eq!(normalize_test("/srv/public", "/foo/../../etc/passwd"), None);
    }

    #[test]
    fn normalize_traversal_deep() {
        assert_eq!(
            normalize_test("/srv/public", "/foo/bar/../../../etc/passwd"),
            None
        );
    }

    #[test]
    fn normalize_benign_parent() {
        assert_eq!(
            normalize_test("/srv", "/foo/../bar"),
            Some(PathBuf::from("/srv/bar"))
        );
    }

    #[test]
    fn normalize_dot_segments() {
        assert_eq!(
            normalize_test("/srv", "/./foo/./bar"),
            Some(PathBuf::from("/srv/foo/bar"))
        );
    }

    #[test]
    fn normalize_strips_query() {
        assert_eq!(
            normalize_test("/srv", "/foo?v=123"),
            Some(PathBuf::from("/srv/foo"))
        );
    }

    #[test]
    fn normalize_reuses_buffer() {
        let base = Path::new("/srv");
        let mut fp = PathBuf::new();
        normalize_into(base, "/first", &mut fp);
        let ptr1 = fp.as_os_str().as_encoded_bytes().as_ptr();
        normalize_into(base, "/second", &mut fp);
        let ptr2 = fp.as_os_str().as_encoded_bytes().as_ptr();
        assert_eq!(ptr1, ptr2);
    }

    // --- mime --------------------------------------------------------------------

    #[test]
    fn mime_known() {
        assert_eq!(mime(Path::new("f.html")), "text/html; charset=utf-8");
        assert_eq!(mime(Path::new("f.css")), "text/css");
        assert_eq!(mime(Path::new("f.js")), "application/javascript");
        assert_eq!(mime(Path::new("f.png")), "image/png");
        assert_eq!(mime(Path::new("f.pdf")), "application/pdf");
    }

    #[test]
    fn mime_unknown() {
        assert_eq!(mime(Path::new("f.xyz")), "application/octet-stream");
        assert_eq!(mime(Path::new("noext")), "application/octet-stream");
    }

    #[test]
    fn markdown_extension_detection() {
        assert!(is_markdown(Path::new("README.md")));
        assert!(is_markdown(Path::new("README.Markdown")));
        assert!(!is_markdown(Path::new("README.txt")));
        assert!(!is_markdown(Path::new("README")));
    }

    // --- Integration tests (real TCP) --------------------------------------------

    struct TestServer {
        addr: std::net::SocketAddr,
        _dir: tempfile::TempDir,
        shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl TestServer {
        fn new() -> Self {
            Self::new_with_docs(false)
        }

        fn new_docs() -> Self {
            Self::new_with_docs(true)
        }

        fn new_with_docs(docs_mode: bool) -> Self {
            let dir = tempfile::tempdir().unwrap();

            fs::write(dir.path().join("hello.txt"), "hello world").unwrap();
            fs::write(dir.path().join("style.css"), "body{}").unwrap();
            fs::write(
                dir.path().join("README.md"),
                "# Hello\n\nSome *markdown* text.\n\n- [x] one\n- [ ] two\n\n|a|b|\n|-|-|\n|1|2|\n",
            )
            .unwrap();
            fs::write(dir.path().join(".gitignore"), "ignored/\n").unwrap();
            fs::create_dir(dir.path().join("docs")).unwrap();
            fs::write(dir.path().join("docs").join("guide.md"), "# Guide").unwrap();
            fs::create_dir(dir.path().join("ignored")).unwrap();
            fs::write(dir.path().join("ignored").join("skip.md"), "# Skip").unwrap();
            fs::create_dir(dir.path().join(".hidden")).unwrap();
            fs::write(dir.path().join(".hidden").join("secret.md"), "# Secret").unwrap();
            fs::create_dir(dir.path().join("sub")).unwrap();
            fs::write(dir.path().join("sub").join("index.html"), "<h1>hi</h1>").unwrap();
            fs::write(dir.path().join("file 2.txt"), "spaced").unwrap();
            fs::write(dir.path().join("a1.txt"), "").unwrap();
            fs::write(dir.path().join("a10.txt"), "").unwrap();
            fs::write(dir.path().join("a2.txt"), "").unwrap();

            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let shut = shutdown.clone();

            let mut srv = Server {
                dir: fs::canonicalize(dir.path()).unwrap(),
                quiet: true,
                docs_mode,
                fp: PathBuf::new(),
                html: String::new(),
            };

            let thread = std::thread::spawn(move || {
                listener.set_nonblocking(true).unwrap();
                while !shut.load(std::sync::atomic::Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            stream.set_nonblocking(false).unwrap();
                            let _ = srv.handle(stream);
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                            std::thread::sleep(std::time::Duration::from_millis(5));
                        }
                        Err(_) => break,
                    }
                }
            });

            TestServer {
                addr,
                _dir: dir,
                shutdown,
                thread: Some(thread),
            }
        }

        fn get(&self, path: &str) -> (String, String) {
            let mut stream = TcpStream::connect(self.addr).unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            write!(stream, "GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
            let mut resp = String::new();
            let _ = stream.read_to_string(&mut resp);
            let (headers, body) = resp.split_once("\r\n\r\n").unwrap_or((&resp, ""));
            (headers.to_owned(), body.to_owned())
        }

        fn status(&self, path: &str) -> u16 {
            let (headers, _) = self.get(path);
            headers
                .lines()
                .next()
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(0)
        }

        fn post(&self, path: &str) -> u16 {
            let mut stream = TcpStream::connect(self.addr).unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            write!(stream, "POST {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
            let mut resp = String::new();
            let _ = stream.read_to_string(&mut resp);
            resp.lines()
                .next()
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(0)
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.shutdown
                .store(true, std::sync::atomic::Ordering::Relaxed);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    #[test]
    fn integration_serve_file() {
        let srv = TestServer::new();
        let (headers, body) = srv.get("/hello.txt");
        assert_eq!(srv.status("/hello.txt"), 200);
        assert!(headers.contains("Content-Type: text/plain"));
        assert!(headers.contains("Content-Length: 11"));
        assert!(headers.contains("Cache-Control: no-store"));
        assert!(headers.contains("Connection: close"));
        assert_eq!(body, "hello world");
    }

    #[test]
    fn integration_mime_type() {
        let srv = TestServer::new();
        let (headers, _) = srv.get("/style.css");
        assert!(headers.contains("Content-Type: text/css"));
    }

    #[test]
    fn integration_markdown_renders_to_html() {
        let srv = TestServer::new();
        let (headers, body) = srv.get("/README.md");
        assert!(headers.contains("200 OK"));
        assert!(headers.contains("Content-Type: text/html; charset=utf-8"));
        assert!(body.contains("<article class=\"markdown-body\">"));
        assert!(body.contains("<h1>Hello</h1>"));
        assert!(body.contains("<em>markdown</em>"));
        assert!(body.contains("<table>"));
        assert!(body.contains("<td>1</td>"));
        assert!(body.contains("type=\"checkbox\""));
    }

    #[test]
    fn integration_docs_mode_shell() {
        let srv = TestServer::new_docs();
        let (headers, body) = srv.get("/");
        assert!(headers.contains("200 OK"));
        assert!(headers.contains("Content-Type: text/html; charset=utf-8"));
        assert!(body.contains("id=\"tree-root\""));
        assert!(body.contains("/__docs__/tree"));
    }

    #[test]
    fn integration_docs_mode_tree_filters_hidden_and_gitignored_dirs() {
        let srv = TestServer::new_docs();
        let (headers, body) = srv.get("/__docs__/tree");
        assert!(headers.contains("200 OK"));
        assert!(headers.contains("Content-Type: application/json; charset=utf-8"));
        assert!(body.contains("README.md"));
        assert!(body.contains("guide.md"));
        assert!(!body.contains("skip.md"));
        assert!(!body.contains("secret.md"));
    }

    #[test]
    fn integration_docs_mode_markdown_fragment() {
        let srv = TestServer::new_docs();
        let (headers, body) = srv.get("/__docs__/md?path=README.md");
        assert!(headers.contains("200 OK"));
        assert!(headers.contains("Content-Type: text/html; charset=utf-8"));
        assert!(body.contains("<h1>Hello</h1>"));
        assert!(!body.contains("<html>"));
    }

    #[test]
    fn integration_directory_listing() {
        let srv = TestServer::new();
        let (headers, body) = srv.get("/");
        assert!(headers.contains("200 OK"));
        assert!(headers.contains("text/html"));
        assert!(body.contains("font-family: monospace"));
        assert!(body.contains("<table>"));
        assert!(body.contains("</table>"));
        assert!(body.contains("hello.txt"));
        assert!(body.contains("sub/"));
    }

    #[test]
    fn integration_directory_natural_sort() {
        let srv = TestServer::new();
        let (_, body) = srv.get("/");
        let pos1 = body.find("a1.txt").unwrap();
        let pos2 = body.find("a2.txt").unwrap();
        let pos10 = body.find("a10.txt").unwrap();
        assert!(pos1 < pos2, "a1 should come before a2");
        assert!(pos2 < pos10, "a2 should come before a10");
    }

    #[test]
    fn integration_index_html() {
        let srv = TestServer::new();
        let (headers, body) = srv.get("/sub/");
        assert!(headers.contains("200 OK"));
        assert_eq!(body, "<h1>hi</h1>");
    }

    #[test]
    fn integration_not_found() {
        let srv = TestServer::new();
        assert_eq!(srv.status("/nonexistent"), 404);
    }

    #[test]
    fn integration_method_not_allowed() {
        let srv = TestServer::new();
        assert_eq!(srv.post("/hello.txt"), 405);
    }

    #[test]
    fn integration_path_traversal() {
        let srv = TestServer::new();
        assert_eq!(srv.status("/../../../etc/passwd"), 403);
        assert_eq!(srv.status("/foo/../../etc/passwd"), 403);
    }

    #[test]
    fn integration_encoded_path() {
        let srv = TestServer::new();
        let (_, body) = srv.get("/file%202.txt");
        assert_eq!(body, "spaced");
    }

    #[test]
    fn integration_encoded_traversal() {
        let srv = TestServer::new();
        assert_eq!(srv.status("/%2e%2e/%2e%2e/etc/passwd"), 403);
    }

    #[test]
    fn integration_directory_listing_encodes_filenames() {
        let srv = TestServer::new();
        let (_, body) = srv.get("/");
        assert!(body.contains("file%202.txt"));
        assert!(body.contains("file 2.txt"));
    }
}
