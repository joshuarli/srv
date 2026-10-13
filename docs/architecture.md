# Architecture

The binary is implemented in `src/main.rs`. It canonicalizes the selected file or directory at startup, binds a TCP listener, and handles accepted connections synchronously. Each connection receives one GET request and closes after its response.

For directories, requests serve `index.html` when it exists or generate a naturally sorted listing. Files are served with an extension-based content type; `.md` and `.markdown` files are rendered as HTML. Selecting a file serves that file for every request path.

`--docs` embeds a Markdown browser in the binary. It builds a Markdown file tree on request, omitting hidden directories and directories matched by simple positive `.gitignore` patterns. Markdown is rendered with tables, strikethrough, and task-list extensions.

URL normalization rejects `..` segments that climb above the canonical root. This does not constrain filesystem symlinks: serving paths through symlinked directories can reach outside the selected root, and the docs tree can traverse symlinked directories. Keep symlinks under a served root trusted.
