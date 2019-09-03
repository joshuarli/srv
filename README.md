# srv

minimalist http server and file browser

differences between `python -m http.server`:

- shows file size
- does not follow symlinks
    - by extension, refuses access to all irregular files
- serves some automatically detected Content-Type mimetypes for browser previews, as opposed to plain octet-stream
    - note that this is dependent on go's [DetectContentType](https://golang.org/src/net/http/sniff.go)
- is probably faster (benchmarks soon if i feel like it)


## download

cross-platform static executables can be found [here](https://github.com/joshuarli/srv/releases).
