use std::{
    env,
    fmt::Write as FmtWrite,
    fs::{self, File},
    io::{self, BufWriter, Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::io::AsRawFd,
    path::{Path, PathBuf},
};

const LISTING_PRELUDE: &str =
    "<head><link rel=icon href=data:,><style>* { font-family: monospace; } \
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
fn natural_less(s1: &[u8], s2: &[u8]) -> bool {
    let (mut i1, mut i2) = (0usize, 0usize);
    while i1 < s1.len() && i2 < s2.len() {
        let (c1, c2) = (s1[i1], s2[i2]);
        let (d1, d2) = (c1.is_ascii_digit(), c2.is_ascii_digit());
        if d1 != d2 {
            return d1;
        }
        if !d1 {
            if c1 != c2 {
                return c1 < c2;
            }
            i1 += 1;
            i2 += 1;
        } else {
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
            let (len1, len2) = (i1 - nz1, i2 - nz2);
            if len1 != len2 {
                return len1 < len2;
            }
            let (nr1, nr2) = (&s1[nz1..i1], &s2[nz2..i2]);
            if nr1 != nr2 {
                return nr1 < nr2;
            }
            if nz1 != nz2 {
                return nz1 < nz2;
            }
        }
    }
    s1.len() < s2.len()
}

// --- URL encoding -------------------------------------------------------------

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex_val(b[i + 1]), hex_val(b[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
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

fn normalize(base: &Path, raw: &str) -> Option<PathBuf> {
    let raw = raw.split('?').next().unwrap_or("/");
    let mut out = base.to_path_buf();
    for seg in raw.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    if out.starts_with(base) {
        Some(out)
    } else {
        None
    }
}

// --- MIME types ---------------------------------------------------------------

fn mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
    {
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
        fn sendfile(
            fd: i32,
            s: i32,
            offset: i64,
            len: *mut i64,
            hdtr: *mut (),
            flags: i32,
        ) -> i32;
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
        let n =
            unsafe { sendfile(sock.as_raw_fd(), file.as_raw_fd(), &mut offset, remaining) };
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

// --- HTTP helpers -------------------------------------------------------------

fn respond_error(sock: &TcpStream, code: u16, msg: &str) -> io::Result<()> {
    let body = format!("{code} {msg}");
    let mut w = BufWriter::new(sock);
    write!(
        w,
        "HTTP/1.1 {code} {msg}\r\n\
         Content-Type: text/plain\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    )?;
    w.flush()
}

fn serve_file(sock: &TcpStream, path: &Path, len: u64, content_type: &str) -> io::Result<()> {
    let file = File::open(path)?;
    {
        let mut w = BufWriter::new(sock);
        write!(
            w,
            "HTTP/1.1 200 OK\r\n\
             Content-Type: {content_type}\r\n\
             Content-Length: {len}\r\n\
             Cache-Control: no-store\r\n\
             Connection: close\r\n\
             \r\n"
        )?;
        w.flush()?;
    }
    send_file(&file, sock, len)
}

// --- Directory listing --------------------------------------------------------

fn render_listing(dir: &Path) -> io::Result<String> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
    entries.sort_by(|a, b| {
        let an = a.file_name().to_string_lossy().to_lowercase();
        let bn = b.file_name().to_string_lossy().to_lowercase();
        if natural_less(an.as_bytes(), bn.as_bytes()) {
            std::cmp::Ordering::Less
        } else if natural_less(bn.as_bytes(), an.as_bytes()) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });

    let mut html = String::from(LISTING_PRELUDE);
    for entry in &entries {
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
            write!(html, "<tr><td><p style=\"color: #777\">{name}</p></td></tr>").unwrap();
        }
    }
    html.push_str("</table>");
    Ok(html)
}

// --- Connection handler -------------------------------------------------------

fn handle(mut stream: TcpStream, srv_dir: &Path, quiet: bool) -> io::Result<()> {
    let mut buf = [0u8; 8192];
    let mut pos = 0usize;

    let (method, path) = 'read: {
        loop {
            if pos == buf.len() {
                respond_error(&stream, 431, "Request Header Fields Too Large")?;
                return Ok(());
            }
            let n = stream.read(&mut buf[pos..])?;
            if n == 0 {
                return Ok(());
            }
            pos += n;
            let mut headers = [httparse::EMPTY_HEADER; 16];
            let mut req = httparse::Request::new(&mut headers);
            match req.parse(&buf[..pos]) {
                Ok(httparse::Status::Complete(_)) => {
                    break 'read (
                        req.method.unwrap_or("").to_owned(),
                        req.path.unwrap_or("/").to_owned(),
                    );
                }
                Ok(httparse::Status::Partial) => continue,
                Err(_) => {
                    respond_error(&stream, 400, "Bad Request")?;
                    return Ok(());
                }
            }
        }
    };

    if !quiet {
        let peer = stream
            .peer_addr()
            .map(|a| a.to_string())
            .unwrap_or_default();
        eprintln!("\t{peer}: {method} {path}");
    }

    if method != "GET" {
        respond_error(&stream, 405, "Method Not Allowed")?;
        return Ok(());
    }

    let decoded = percent_decode(&path);
    let fp = match normalize(srv_dir, &decoded) {
        Some(p) => p,
        None => {
            respond_error(&stream, 403, "Forbidden")?;
            return Ok(());
        }
    };

    let meta = match fs::symlink_metadata(&fp) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            respond_error(&stream, 404, "Not Found")?;
            return Ok(());
        }
        Err(e) => return Err(e),
    };

    let ft = meta.file_type();
    if ft.is_dir() {
        let index = fp.join("index.html");
        if let Ok(idx_meta) = fs::metadata(&index) {
            if idx_meta.is_file() {
                return serve_file(
                    &stream,
                    &index,
                    idx_meta.len(),
                    "text/html; charset=utf-8",
                );
            }
        }
        let html = render_listing(&fp)?;
        let mut w = BufWriter::new(&stream);
        write!(
            w,
            "HTTP/1.1 200 OK\r\n\
             Content-Type: text/html; charset=utf-8\r\n\
             Content-Length: {}\r\n\
             Cache-Control: no-store\r\n\
             Connection: close\r\n\
             \r\n",
            html.len()
        )?;
        w.write_all(html.as_bytes())?;
        w.flush()?;
    } else if ft.is_file() {
        serve_file(&stream, &fp, meta.len(), mime(&fp))?;
    } else if ft.is_symlink() {
        respond_error(&stream, 403, "Forbidden: symlinks not served")?;
    } else {
        respond_error(&stream, 403, "Forbidden: not a regular file or directory")?;
    }

    Ok(())
}

// --- main --------------------------------------------------------------------

fn die(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(1);
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut quiet = false;
    let mut port = String::from("8000");
    let mut bind = String::from("127.0.0.1");
    let mut dir = String::from(".");

    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "-q" => quiet = true,
            "-p" => {
                i += 1;
                if i < args.len() {
                    port = args[i].clone();
                }
            }
            "-b" => {
                i += 1;
                if i < args.len() {
                    bind = args[i].clone();
                }
            }
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
            arg => {
                dir = arg.to_owned();
            }
        }
        i += 1;
    }

    let srv_dir = match fs::canonicalize(&dir) {
        Ok(p) => p,
        Err(e) => die(&format!("{dir}: {e}")),
    };
    match fs::metadata(&srv_dir) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => die(&format!("{dir} is not a directory")),
        Err(e) => die(&format!("{dir}: {e}")),
    }

    let addr = format!("{bind}:{port}");
    let listener = TcpListener::bind(&addr)
        .unwrap_or_else(|e| die(&format!("failed to bind {addr}: {e}")));

    eprintln!("\tServing {} over HTTP on {addr}", srv_dir.display());

    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                if let Err(e) = handle(s, &srv_dir, quiet) {
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

    // --- natural_less ------------------------------------------------------------

    #[test]
    fn natural_less_basic() {
        assert!(natural_less(b"a", b"b"));
        assert!(!natural_less(b"b", b"a"));
        assert!(!natural_less(b"a", b"a"));
    }

    #[test]
    fn natural_less_numeric() {
        assert!(natural_less(b"file2", b"file10"));
        assert!(!natural_less(b"file10", b"file2"));
        assert!(natural_less(b"file1", b"file2"));
    }

    #[test]
    fn natural_less_leading_zeros() {
        // equal numeric value, fewer leading zeros sorts first
        assert!(natural_less(b"file01", b"file001"));
    }

    #[test]
    fn natural_less_digits_before_letters() {
        assert!(natural_less(b"1abc", b"abc"));
        assert!(!natural_less(b"abc", b"1abc"));
    }

    #[test]
    fn natural_less_prefix() {
        assert!(natural_less(b"file", b"file1"));
        assert!(!natural_less(b"file1", b"file"));
    }

    #[test]
    fn natural_less_empty() {
        assert!(natural_less(b"", b"a"));
        assert!(!natural_less(b"a", b""));
        assert!(!natural_less(b"", b""));
    }

    // --- percent_decode / percent_encode -----------------------------------------

    #[test]
    fn percent_decode_passthrough() {
        assert_eq!(percent_decode("hello"), "hello");
    }

    #[test]
    fn percent_decode_space() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
    }

    #[test]
    fn percent_decode_slash() {
        assert_eq!(percent_decode("%2Fetc%2Fpasswd"), "/etc/passwd");
    }

    #[test]
    fn percent_decode_invalid_seq() {
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%ZZ"), "%ZZ");
        assert_eq!(percent_decode("%0"), "%0");
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
        assert_eq!(percent_decode(&percent_encode(input)), input);
    }

    // --- normalize ---------------------------------------------------------------

    #[test]
    fn normalize_root() {
        let base = Path::new("/srv");
        assert_eq!(normalize(base, "/"), Some(PathBuf::from("/srv")));
    }

    #[test]
    fn normalize_subpath() {
        let base = Path::new("/srv");
        assert_eq!(
            normalize(base, "/foo/bar"),
            Some(PathBuf::from("/srv/foo/bar"))
        );
    }

    #[test]
    fn normalize_traversal_basic() {
        let base = Path::new("/srv/public");
        assert_eq!(normalize(base, "/../../../etc/passwd"), None);
    }

    #[test]
    fn normalize_traversal_partial() {
        let base = Path::new("/srv/public");
        assert_eq!(normalize(base, "/foo/../../etc/passwd"), None);
    }

    #[test]
    fn normalize_traversal_deep() {
        let base = Path::new("/srv/public");
        assert_eq!(
            normalize(base, "/foo/bar/../../../etc/passwd"),
            None
        );
    }

    #[test]
    fn normalize_benign_parent() {
        let base = Path::new("/srv");
        assert_eq!(
            normalize(base, "/foo/../bar"),
            Some(PathBuf::from("/srv/bar"))
        );
    }

    #[test]
    fn normalize_dot_segments() {
        let base = Path::new("/srv");
        assert_eq!(
            normalize(base, "/./foo/./bar"),
            Some(PathBuf::from("/srv/foo/bar"))
        );
    }

    #[test]
    fn normalize_strips_query() {
        let base = Path::new("/srv");
        assert_eq!(
            normalize(base, "/foo?v=123"),
            Some(PathBuf::from("/srv/foo"))
        );
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

    /// Spin up the server on an OS-assigned port, returning the listener address.
    /// The server runs in a background thread handling one request per call to
    /// `accept`. Returns (addr, join_handle_dropper).
    struct TestServer {
        addr: std::net::SocketAddr,
        _dir: tempfile::TempDir,
        shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl TestServer {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();

            // Populate fixture files
            fs::write(dir.path().join("hello.txt"), "hello world").unwrap();
            fs::write(dir.path().join("style.css"), "body{}").unwrap();
            fs::create_dir(dir.path().join("sub")).unwrap();
            fs::write(dir.path().join("sub").join("index.html"), "<h1>hi</h1>").unwrap();
            fs::write(dir.path().join("file 2.txt"), "spaced").unwrap();
            fs::write(dir.path().join("a1.txt"), "").unwrap();
            fs::write(dir.path().join("a10.txt"), "").unwrap();
            fs::write(dir.path().join("a2.txt"), "").unwrap();

            let srv_dir = fs::canonicalize(dir.path()).unwrap();
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let shut = shutdown.clone();

            let thread = std::thread::spawn(move || {
                listener.set_nonblocking(true).unwrap();
                while !shut.load(std::sync::atomic::Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            stream.set_nonblocking(false).unwrap();
                            let _ = handle(stream, &srv_dir, true);
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
        // Check the prelude styling is present
        assert!(body.contains("font-family: monospace"));
        assert!(body.contains("<table>"));
        assert!(body.contains("</table>"));
        // Files and dirs should appear
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
        // "file 2.txt" should appear percent-encoded in the href
        assert!(body.contains("file%202.txt"));
        // but the display name should be readable
        assert!(body.contains("file 2.txt"));
    }
}
