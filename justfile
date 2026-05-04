build-all:
    cargo cross build --target x86_64-unknown-linux-gnu --release
    cargo cross build --target x86_64-pc-windows-gnu --release
    cargo cross build --target x86_64-apple-darwin --release
