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

const LISTING_PRELUDE: &str = "<head><link rel=icon href=data:,><style>* { font-family: monospace; } \
     table { border: none; margin: 1rem; } td { padding-right: 2rem; }</style></head>\n\
     <table>";

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

fn serve_file(sock: &TcpStream, path: &Path, len: u64, content_type: &str) -> io::Result<()> {
    let file = File::open(path)?;
    write_headers(sock, 200, "OK", content_type, len)?;
    send_file(&file, sock, len)
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
                let result =
                    serve_file(sock, &self.fp, idx_meta.len(), "text/html; charset=utf-8");
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
            serve_file(sock, &self.fp, meta.len(), mime(&self.fp))?;
        } else if ft.is_symlink() {
            write_error(sock, 403, "Forbidden: symlinks not served")?;
        } else {
            write_error(sock, 403, "Forbidden: not a regular file or directory")?;
        }

        Ok(())
    }
}

// --- main --------------------------------------------------------------------

fn die(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(1);
}

fn main() {
    let mut quiet = false;
    let mut port = String::from("8000");
    let mut bind = String::from("127.0.0.1");
    let mut dir = String::from(".");

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-q" => quiet = true,
            "-p" => port = args.next().unwrap_or_else(|| die("-p requires a value")),
            "-b" => bind = args.next().unwrap_or_else(|| die("-b requires a value")),
            "-h" | "--help" => {
                eprintln!(
                    "usage: srv [-q] [-p port] [-b address] [directory]\n\
                     \n\
                     directory    path to serve (default: .)\n\
                     -q           quiet; disable logging\n\
                     -p port      port to listen on (default: 8000)\n\
                     -b address   bind address (default: 127.0.0.1)"
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

    // --- Integration tests (real TCP) --------------------------------------------

    struct TestServer {
        addr: std::net::SocketAddr,
        _dir: tempfile::TempDir,
        shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl TestServer {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();

            fs::write(dir.path().join("hello.txt"), "hello world").unwrap();
            fs::write(dir.path().join("style.css"), "body{}").unwrap();
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
