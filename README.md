# srv

minimalist http(s) server and file browser.

<img src="https://github.com/joshuarli/srv/blob/master/.github/screenshot.png?raw=true" width="200">


## download

static executables for some platforms can be found [here](https://github.com/joshuarli/srv/releases).


## usage

Simply `srv`. Defaults are `-p 8000 -b 127.0.0.1 -d .`


## usage: TLS

TLS and HTTP/2 are enabled if you pass `-c certfile -k keyfile`.

to make self-signed certs:

    openssl req -nodes -new -x509 -keyout key.pem -out cert.pem -subj "/"

or better, locally trusted certs with [mkcert](https://github.com/FiloSottile/mkcert):

    mkcert -install
    mkcert -key-file key.pem -cert-file cert.pem -ecdsa 127.0.0.1
