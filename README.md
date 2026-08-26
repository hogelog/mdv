# mdv

A small local Markdown viewer. A background daemon serves files only on
`127.0.0.1`.

## Install

```sh
cargo install --path .
```

## Use

```sh
mdv README.md          # start the daemon if needed and open the file
mdv                    # open the recently-viewed page
mdv --start            # explicitly start the daemon
mdv --stop             # stop it
mdv --start --port 9000
```

The default URL is <http://localhost:8088/>. Absolute Markdown paths are mapped
directly to URLs, for example:

```text
http://localhost:8088/Users/me/project/README.md
```

History and daemon metadata are stored in `~/.config/mdv`, or in
`$XDG_CONFIG_HOME/mdv` when `XDG_CONFIG_HOME` is set.
