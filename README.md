# windows-notice

Pops up a Windows notification (text + optional image) on another computer over the network.

## On the target machine

```
cargo run --release --bin receiver
```

Leave it running. Allow it through Windows Firewall when asked.

## On the sender machine

Copy `notice.example.toml` to `notice.toml`, set `target` to the target machine's IP, then:

```
cargo run --release
```

Override anything from the command line:

```
cargo run --release -- --message "Hello!" --image C:/pics/cat.png
```
