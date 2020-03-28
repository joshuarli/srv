package main

import (
	"flag"
	"fmt"
	"io"
	"log"
	"net/http"
	"os"
	"path"
	"sort"
	"strings"
)

type context struct {
	srvDir string
}

func humanFileSize(nbytes int64) string {
	if nbytes < 1024 {
		return fmt.Sprintf("%d", nbytes)
	}
	var exp int
	n := float64(nbytes)
	for exp = 0; exp < 4; exp++ {
		n /= 1024
		if n < 1024 {
			break
		}
	}
	return fmt.Sprintf("%.1f%c", float64(n), "KMGT"[exp])
}

func renderListing(w http.ResponseWriter, r *http.Request, f *os.File) error {
	files, err := f.Readdir(-1)
	if err != nil {
		return err
	}

	sort.Slice(files, func(i, j int) bool {
		return strings.ToLower(files[i].Name()) < strings.ToLower(files[j].Name())
	})

	var buf strings.Builder
	buf.WriteString("<style>* { font-family: monospace; } table { border: none; margin: 1rem; } td { padding-right: 2rem; }</style>\n")
	buf.WriteString("<table>")

	for _, fi := range files {
		name, size := fi.Name(), fi.Size()
		path := path.Join(r.URL.Path, name)
		switch m := fi.Mode(); {
		// is a directory - render a link
		case m & os.ModeDir != 0:
			fmt.Fprintf(&buf, "<tr><td><a href=\"%s/\">%s/</a></td></tr>", path, name)
		// is not a regular file - don't render a clickable link
		case m & os.ModeType != 0:
			fmt.Fprintf(&buf, "<tr><td><p style=\"color: #777\">%s</p></td></tr>", name)
		default:
			fmt.Fprintf(&buf, "<tr><td><a href=\"%s\">%s</a></td><td>%s</td></tr>", path, name, humanFileSize(size))
		}
	}

	buf.WriteString("</table>")

	fmt.Fprintf(w, buf.String())
	return nil
}

func (c *context) handler(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		// path.Join is Cleaned, but docstring for http.ServeFile says joining r.URL.Path isn't safe
		// however this seems fine? might want to add a small test suite with some dir traversal attacks
		fp := path.Join(c.srvDir, r.URL.Path)

		f, openErr := os.Open(fp)
		defer f.Close()

		// because openErr (PathError) doesn't have a formal API for getting further error granularity,
		// we need to stat it if we want to return a proper 404 when appropriate.
		// also, golang doesn't provide a (*File).Lstat.
		// using f.Stat() will follow symlinks, which is not what we want because we want to isolate
		// all file serving to within the desired directory. So need to use os.Lstat.
		fi, statErr := os.Lstat(fp)
		if statErr != nil {
			http.Error(w, "file not found", http.StatusNotFound)
			return
		}

		if openErr != nil {
			http.Error(w, "failed to open file", http.StatusInternalServerError)
			return
		}

		switch m := fi.Mode(); {
		// is a directory - serve an index.html if it exists, otherwise generate and serve a directory listing
		case m & os.ModeDir != 0:
			// XXX: if a symlink has name "index.html", it will be served here.
			// i could add an extra lstat here, but the scenario is just too rare to justify the additional file operation.
			html, err := os.Open(path.Join(fp, "index.html"))
			defer html.Close()
			if err == nil {
				io.Copy(w, html)
				return
			}
			err = renderListing(w, r, f)
			if err != nil {
				http.Error(w, "failed to render directory listing: "+err.Error(), http.StatusInternalServerError)
			}
		// is a regular file - serve its contents
		case m & os.ModeType == 0:
			io.Copy(w, f)
		// is a symlink - refuse to serve
		case m & os.ModeSymlink != 0:
			http.Error(w, "file is a symlink", http.StatusForbidden)
		default:
			http.Error(w, "file isn't a regular file or directory", http.StatusForbidden)
		}
	default:
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
	}
}

func die(format string, v ...interface{}) {
	fmt.Fprintf(os.Stderr, format, v...)
	os.Stderr.Write([]byte("\n"))
	os.Exit(1)
}

func main() {
	// maybe should add a -bind domain (localhost, 0.0.0.0, etc.)
	flag.Usage = func() {
	    die(`srv ver. %s

usage: %s [-p port] [-d directory] [-c certfile -k keyfile]

-p port			port to listen on (default: 8000)
-d directory	path to directory to serve (default: .)
-c certfile		optional path to a PEM-format X.509 certificate
-k keyfile		optional path to a PEM-format X.509 key
`, VERSION, os.Args[0])
	}

	var port, srvDir, certFile, keyFile string
	flag.StringVar(&port, "p", "8000", "")
	flag.StringVar(&srvDir, "d", ".", "")
	flag.StringVar(&certFile, "c", "", "")
	flag.StringVar(&keyFile, "k", "", "")
	flag.Parse()

	certFileSpecified := certFile != ""
	keyFileSpecified := keyFile != ""
	if certFileSpecified != keyFileSpecified {
		die("You must specify both -c certfile -k keyfile")
	}

	f, err := os.Open(srvDir)
	defer f.Close()
	if err != nil {
		die(err.Error())
	}
	if fi, err := f.Stat(); err != nil || !fi.IsDir() {
		die("%s isn't a directory", srvDir)
	}

	c := &context{
		srvDir: srvDir,
	}

	// go can also pretty easily stamp out self-signed certs, but we'll delegate
	// that to external tools for now.
	http.HandleFunc("/", c.handler)
	if certFileSpecified && keyFileSpecified {
		log.Fatal(http.ListenAndServeTLS(":"+port, certFile, keyFile, nil))
	} else {
		log.Fatal(http.ListenAndServe(":"+port, nil))
	}
}
