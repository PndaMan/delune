# From source

You need [Rust](https://rustup.rs) 1.90 or newer and [Bun](https://bun.sh).

```sh
git clone https://github.com/PndaMan/delune && cd delune
(cd web && bun install && bun run build)     # the web app, embedded at compile time
cargo build --release -p delune -p delune-tui
./target/release/delune setup
```

Or straight from Git with Cargo (it builds the web app for you only if `web/dist`
exists, so prefer the steps above):

```sh
cargo install --git https://github.com/PndaMan/delune delune delune-tui
```

## A portable Linux binary

Release builds for Linux are static (musl), so one file runs on any distribution:

```sh
rustup target add x86_64-unknown-linux-musl
sudo apt install musl-tools        # or your distribution's musl-gcc
cargo build --release --target x86_64-unknown-linux-musl -p delune -p delune-tui
```

## Developing

`scripts/dev.sh` runs the API server and the hot-reloading web app together; open
<http://localhost:5173>. See [Contributing](../project/contributing.md).
