package main

import (
	"fmt"
	"io"
	"log"
	"net/http"
	"os"
	"path"
)

type context struct {
	srvDir string
}

func renderListing(w http.ResponseWriter, r *http.Request, f *os.File) error {
	files, err := f.Readdir(-1)
	if err != nil {
		return err
	}
    fmt.Fprintf(w, "<style>html { font-family: monospace; } table { border: none; margin: 1rem; } td { padding-right: 2rem; }</style>\n")
    fmt.Fprintf(w, "<table>")
	for _, fi := range files {
        fmt.Fprintf(w, "<tr>")
        // TODO: separately sort hidden files? probably an optional feature, would make code more complicated
		name, size := fi.Name(), fi.Size()
        path := path.Join(r.URL.Path, name)
		switch {
        // TODO: css ellipsis e.g. text-overflow: ellipsis;
		case fi.IsDir():
			fmt.Fprintf(w, "<td><a href=\"%s/\">%s/</a></td>\n", path, name)
		default:
			fmt.Fprintf(w, "<td><a href=\"%s\">%s</a></td><td>%d</td>\n", path, name, size)
		}
        fmt.Fprintf(w, "</tr>")
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

        // TODO: what if a trailing slash is requested? does the above remove that, or if not and if it errors, then strip it (could be more than one////)
		// can this be removed? i think i was using this to stop symlinks, still need to test
        fi, err := os.Lstat(fp)
		if err != nil {
			http.Error(w, "file not found" + err.Error(), http.StatusNotFound)
			return
		}

		f, err := os.OpenFile(fp, os.O_RDONLY, 0444)
		defer f.Close()
		if err != nil {
			http.Error(w, "failed to open file", http.StatusNotFound)
			return
		}

		switch {
		case fi.IsDir():
            html, err := os.OpenFile(path.Join(fp, "index.html"), os.O_RDONLY, 0444)
            defer html.Close()
            if err == nil {
                io.Copy(w, html)
                return
            }
			// TODO: when creating index.html, make symlinks unclickable (what does python server do for symlinks?) - detect via Lstat then FileMode IsSymlink (write my own based on https://golang.org/src/os/types.go?s=3303:3333#L83)
			err = renderListing(w, r, f)
			if err != nil {
				http.Error(w, "failed to render directory listing: "+err.Error(), http.StatusInternalServerError)
			}
		case fi.Mode().IsRegular():
			io.Copy(w, f)
		default:
			http.Error(w, "file isn't a regular file or directory", http.StatusBadRequest)  // statusbadrequest isn't semantically correct here i dont think
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
`, "0.0", os.Args[0])
	}
	port := os.Args[1]

	c := &context{
		srvDir: srvDir,
	}
    http.HandleFunc("/", c.handler)
	log.Fatal(http.ListenAndServe(":"+port, nil))
}
