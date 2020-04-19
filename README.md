# srv

minimalist http(s) server and file browser.

i wrote this to substitute `python3 -m http.server`. here are the differences:

- shows file size
- does not follow symlinks
    - by extension, refuses access to all irregular files
- by default, tells the client to NOT cache responses
- serves some automatically detected Content-Type mimetypes for browser previews, as opposed to plain octet-stream
    - note that this is dependent on go's [DetectContentType](https://golang.org/src/net/http/sniff.go)
- is much faster (see [benchmarks](#benchmarks), mostly in part due to golang and some of my own choices


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

srv 12ba67bf7 `srv -q`

    $ ./bench
    Running 5s test @ http://127.0.0.1:8000
      8 threads and 8 connections
      Thread Stats   Avg      Stdev     Max   +/- Stdev
        Latency   646.27us    1.28ms  17.23ms   93.00%
        Req/Sec     2.94k   548.07     6.29k    75.43%
      117690 requests in 5.10s, 98.54MB read
    Requests/sec:  23084.10
    Transfer/sec:     19.33MB
    wrk is done. response code counts:
    200     117690
