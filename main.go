package main

import (
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

	fmt.Fprintf(w, "<style>* { font-family: monospace; } table { border: none; margin: 1rem; } td { padding-right: 2rem; }</style>\n")
	fmt.Fprintf(w, "<table>")

	for _, fi := range files {
		name, size := fi.Name(), fi.Size()
		path := path.Join(r.URL.Path, name)
		switch {
		case fi.IsDir():
			fmt.Fprintf(w, "<tr><td><a href=\"%s/\">%s/</a></td></tr>", path, name)
		case !fi.Mode().IsRegular():
			fmt.Fprintf(w, "<tr><td><p style=\"color: #777\">%s</p></td></tr>", name)
		default:
			fmt.Fprintf(w, "<tr><td><a href=\"%s\">%s</a></td><td>%s</td></tr>", path, name, humanFileSize(size))
		}
	}

	fmt.Fprintf(w, "</table>")
	return nil
}

func (c *context) handler(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		// path.Join is Cleaned, but docstring for http.ServeFile says joining r.URL.Path isn't safe
		// however this seems fine? might want to add a small test suite with some dir traversal attacks
		fp := path.Join(c.srvDir, r.URL.Path)

		fi, err := os.Lstat(fp)
		if err != nil {
			http.Error(w, "file not found", http.StatusNotFound)
			return
		}

		f, err := os.Open(fp)
		defer f.Close()
		if err != nil {
			http.Error(w, "failed to open file", http.StatusInternalServerError)
			return
		}

		switch {
		case fi.IsDir():
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
		case fi.Mode().IsRegular():
			io.Copy(w, f)
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
	argv := len(os.Args)
	var srvDir string
	switch {
	case argv == 3:
		srvDir = os.Args[2]
		f, err := os.Open(srvDir)
		defer f.Close()
		if err != nil {
			die(err.Error())
		}
		if fi, err := f.Stat(); err != nil || !fi.IsDir() {
			die("%s isn't a directory", srvDir)
		}
	case argv == 2:
		var exists bool
		srvDir, exists = os.LookupEnv("PWD")
		if !exists {
			die("PWD is not set, cannot infer directory.")
		}
	default:
		die(`srv ver. %s

usage: %s port [directory]

directory	path to directory to serve (default: PWD)
`, VERSION, os.Args[0])
	}
	port := os.Args[1]

	c := &context{
		srvDir: srvDir,
	}
	http.HandleFunc("/", c.handler)
	log.Fatal(http.ListenAndServe(":"+port, nil))
}
