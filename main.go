package main

import (
	"fmt"
	"log"
	"net/http"
	"os"
)

type context struct {
	srvDir string
}

func (c *context) handler(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		// https://golang.org/src/net/http/fs.go?s=19279:19336#L660
		// ServeFile does a lot of what we want, i'm basically just going to reskin dirList
		// don't forget to vendor the security stuff like containsDotDot
		// can simplify serveContent a lot as well, in particular only take the mime type code
		// to write the header and then just io.Copy the file body
		http.ServeFile(w, r, c.srvDir + r.URL.Path)
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
