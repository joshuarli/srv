# srv

minimalist http server and file browser

differences between `python -m http.server`:

- shows file size
- does not follow symlinks
    - by extension, refuses access to all irregular files
- serves content-type (golang builtin, TODO decide if i want this or disable it) as opposed to octet-stream
- is probably faster
