package main

import (
	"flag"
	"fmt"
	"html"
	"io"
	"io/ioutil"
	"log"
	"net"
	"net/http"
	"os"
	"path"
	"runtime"
	"sort"
	"strings"

	"github.com/joshuarli/srv/internal/humanize"
)

type context struct {
	srvDir string
}

// We write the shortest browser-valid base64 data string,
// so that the browser does not request the favicon.
const listingPrelude = `<head><link rel=icon href=data:,><style>* { font-family: monospace; } table { border: none; margin: 1rem; } td { padding-right: 2rem; }</style></head>
<table>`

func renderListing(w http.ResponseWriter, r *http.Request, f *os.File) error {
	files, err := f.Readdir(-1)
	if err != nil {
		return err
	}

	io.WriteString(w, listingPrelude)

	sort.Slice(files, func(i, j int) bool {
		// TODO: add switch to make case sensitive
		// TODO: add switch to disable natural sort
		return humanize.NaturalLess(
			strings.ToLower(files[i].Name()),
			strings.ToLower(files[j].Name()),
		)
	})

	var fn, fp string
	for _, fi := range files {
		fn = html.EscapeString(fi.Name())
		fp = path.Join(r.URL.Path, fn)
		switch m := fi.Mode(); {
		// is a directory - render a link
		case m&os.ModeDir != 0:
			fmt.Fprintf(w, "<tr><td><a href=\"%s/\">%s/</a></td></tr>", fp, fn)
		// is a regular file - render both a link and a file size
		case m&os.ModeType == 0:
			fs := humanize.FileSize(fi.Size())
			fmt.Fprintf(w, "<tr><td><a href=\"%s\">%s</a></td><td>%s</td></tr>", fp, fn, fs)
		// otherwise, don't render a clickable link
		// TODO: render symlink dests
		default:
			fmt.Fprintf(w, "<tr><td><p style=\"color: #777\">%s</p></td></tr>", fn)
		}
	}

	io.WriteString(w, "</table>")
	return nil
}

func (c *context) handler(w http.ResponseWriter, r *http.Request) {
	// TODO: better log styling
	log.Printf("\t%s [%s]: %s %s %s", r.RemoteAddr, r.UserAgent(), r.Method, r.Proto, r.Host+r.RequestURI)

	// Tell HTTP 1.1+ clients to not cache responses.
	w.Header().Set("Cache-Control", "no-store")

	switch r.Method {
	case http.MethodGet:
		// path.Join is Cleaned, but docstring for http.ServeFile says joining r.URL.Path isn't safe
		// however this seems fine? might want to add a small test suite with some dir traversal attacks
		fp := path.Join(c.srvDir, r.URL.Path)

		fi, err := os.Lstat(fp)
		if err != nil {
			// NOTE: errors.Is is generally preferred, since it can unwrap errors created like so:
			//     fmt.Errorf("can't read file: %w", err)
			// But in this case we just want to check right after a stat.
			if os.IsNotExist(err) {
				http.Error(w, "file not found", http.StatusNotFound)
				return
			}
			http.Error(w, fmt.Sprintf("failed to stat file: %s", err), http.StatusInternalServerError)
			return
		}

		f, err := os.Open(fp)
		if err != nil {
			http.Error(w, fmt.Sprintf("failed to open file: %s", err), http.StatusInternalServerError)
			return
		}
		defer f.Close()

		switch m := fi.Mode(); {
		// is a directory - serve an index.html if it exists, otherwise generate and serve a directory listing
		case m&os.ModeDir != 0:
			// XXX: if a symlink has name "index.html", it will be served here.
			// i could add an extra lstat here, but the scenario is just too rare
			// to justify the additional file operation.
			html, err := os.Open(path.Join(fp, "index.html"))
			if err == nil {
				io.Copy(w, html)
				html.Close()
				return
			}
			html.Close()
			err = renderListing(w, r, f)
			if err != nil {
				http.Error(w, "failed to render directory listing: "+err.Error(), http.StatusInternalServerError)
			}
		// is a regular file - serve its contents
		case m&os.ModeType == 0:
			io.Copy(w, f)
		// is a symlink - refuse to serve
		case m&os.ModeSymlink != 0:
			// TODO: add a flag to allow serving symlinks
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

// VERSION passed at build time
var VERSION = "unknown"

func main() {
	flag.Usage = func() {
		die(`srv %s (go version %s)

usage: %s [-q] [-p port] [-c certfile -k keyfile] directory

directory       path to directory to serve (default: .)

-q              quiet; disable all logging
-p port         port to listen on (default: 8000)
-b address      listener socket's bind address (default: 127.0.0.1)
-c certfile     optional path to a PEM-format X.509 certificate
-k keyfile      optional path to a PEM-format X.509 key
`, VERSION, runtime.Version(), os.Args[0])
	}

	var quiet bool
	var port, bindAddr, certFile, keyFile string
	flag.BoolVar(&quiet, "q", false, "")
	flag.StringVar(&port, "p", "8000", "")
	flag.StringVar(&bindAddr, "b", "127.0.0.1", "")
	flag.StringVar(&certFile, "c", "", "")
	flag.StringVar(&keyFile, "k", "", "")
	flag.Parse()

	certFileSpecified := certFile != ""
	keyFileSpecified := keyFile != ""
	if certFileSpecified != keyFileSpecified {
		die("You must specify both -c certfile -k keyfile.")
	}

	listenAddr := net.JoinHostPort(bindAddr, port)
	_, err := net.ResolveTCPAddr("tcp", listenAddr)
	if err != nil {
		die("Could not resolve the address to listen to: %s", listenAddr)
	}

	srvDir := "."
	posArgs := flag.Args()
	if len(posArgs) > 0 {
		srvDir = posArgs[0]
	}
	f, err := os.Open(srvDir)
	if err != nil {
		die(err.Error())
	}
	defer f.Close()
	if fi, err := f.Stat(); err != nil || !fi.IsDir() {
		die("%s isn't a directory.", srvDir)
	}

	c := &context{
		srvDir: srvDir,
	}

	if quiet {
		log.SetFlags(0) // disable log formatting to save cpu
		log.SetOutput(ioutil.Discard)
	}

	http.HandleFunc("/", c.handler)

	if certFileSpecified && keyFileSpecified {
		log.Printf("\tServing %s over HTTPS on %s", srvDir, listenAddr)
		err = http.ListenAndServeTLS(listenAddr, certFile, keyFile, nil)
	} else {
		log.Printf("\tServing %s over HTTP on %s", srvDir, listenAddr)
		err = http.ListenAndServe(listenAddr, nil)
	}

	die(err.Error())
}
