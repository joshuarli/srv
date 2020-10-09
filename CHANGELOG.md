# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).


## [0.5] - Unreleased

Nothing planned at the moment.


## [0.4] - 2020-10-09
### Changed
- Mimetypes are now inferred firstly by file extension, and go's DetectContentType failing that.

### Fixed
- As a result of the mimetype inference change mentioned above, CSS files for example will now be served with the appropriate text/css mimetype.

## [0.3] - 2020-08-16
### Added
- Usage now shows Go's runtime version. Also builds with 0.15.

### Changed
- User Agent strings are now logged.

### Fixed
- Links to filenames with quotes are now html-escaped so they work.

## [0.2] - 2020-06-05
### Added
- Custom bind address with `-b address`.
- Optional TLS with `-c certfile -k keyfile`.

### Changed
- Directory entries are now naturally/alphanumerically sorted.
- Symlinks were made forbidden.
- Sends `Cache-Control: no-store` for HTTP 1.1+ clients that obey it (pretty much all major browsers).
- Rendering performance and size was improved.
- Browsers should not request favicons anymore.

## 0.1 - 2019-09-03
Initial release.


[0.5]: https://github.com/joshuarli/srv/compare/0.5...HEAD
[0.4]: https://github.com/joshuarli/srv/compare/0.3...0.4
[0.3]: https://github.com/joshuarli/srv/compare/0.2...0.3
[0.2]: https://github.com/joshuarli/srv/compare/0.1...0.2
