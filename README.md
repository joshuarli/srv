# srv

minimalist http(s) server and file browser.

i wrote this to substitute `python3 -m http.server`. here are the differences:

- shows file size
- does not follow symlinks
    - by extension, refuses access to all irregular files
- serves some automatically detected Content-Type mimetypes for browser previews, as opposed to plain octet-stream
    - note that this is dependent on go's [DetectContentType](https://golang.org/src/net/http/sniff.go)
- by default, tells the client to NOT cache responses
- TLS support
- is nearly 40x faster (see [benchmarks](#benchmarks), mostly in part due to golang and some of my own choices

you _could_ achieve some of these bullet points (e.g. TLS support, response caching) by wrapping python HTTPServer and only using python stdlib, but that hurts from a default usability perspective.


## download

cross-platform static executables can be found [here](https://github.com/joshuarli/srv/releases).


## usage

Simply `srv`. Defaults are `-p 8000 -b 127.0.0.1 -d .`

TLS and HTTP/2 are enabled if you pass `-c certfile -k keyfile`.

self-signed certs:

    openssl req -nodes -new -x509 -keyout key.pem -out cert.pem -subj "/"

or better, locally trusted certs with [mkcert](https://github.com/FiloSottile/mkcert):

    mkcert -install
    mkcert -key-file key.pem -cert-file cert.pem -ecdsa 127.0.0.1


## benchmarks

Python 3.7.3 `python3 -m http.server &>/dev/null`

    $ ./bench
    Running 5s test @ http://127.0.0.1:8000
      8 threads and 8 connections
      Thread Stats   Avg      Stdev     Max   +/- Stdev
        Latency    12.52ms    2.82ms  20.32ms   81.22%
        Req/Sec    79.17     41.95     0.91k    99.75%
      3174 requests in 5.10s, 2.56MB read
    Requests/sec:    622.41
    Transfer/sec:    513.67KB
    wrk is done. response code counts:
    200     3174

...not to mention the spew of `BrokenPipeError: [Errno 32] Broken pipe` towards the end.

srv 0.2 (fully static linux-amd64 build, go1.13.5) `srv -q`

    $ ./bench
    Running 5s test @ http://127.0.0.1:8000
      8 threads and 8 connections
      Thread Stats   Avg      Stdev     Max   +/- Stdev
        Latency   648.80us    1.32ms  18.44ms   93.26%
        Req/Sec     2.96k   489.96     4.82k    72.59%
      119382 requests in 5.10s, 99.62MB read
    Requests/sec:  23409.69
    Transfer/sec:     19.53MB
    wrk is done. response code counts:
    200     119382
