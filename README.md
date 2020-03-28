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


## usage

Simply `srv`. Defaults are `-d . -p 8000`.

TLS and HTTP/2 are enabled if you pass `-c certfile -k keyfile`.

self-signed certs:

    openssl req -nodes -new -x509 -keyout key.pem -out cert.pem -subj "/"

or better, locally trusted certs with [mkcert](https://github.com/FiloSottile/mkcert):

    mkcert -install
    mkcert -key-file key.pem -cert-file cert.pem -ecdsa 127.0.0.1
