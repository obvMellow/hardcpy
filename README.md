
[![Build hardcpy](https://github.com/obvMellow/hardcpy/actions/workflows/rust.yml/badge.svg)](https://github.com/obvMellow/hardcpy/actions/workflows/rust.yml)

# hardcpy
Simple backup tool written in Rust

# Installation

## Prebuilt Binary

Install the latest binary from [Releases](https://github.com/obvMellow/hardcpy/releases)

### AUR

On Arch Linux you can install the pre-built binary from the AUR
```sh
git clone https://aur.archlinux.org/hardcpy-bin.git
cd hardcpy-bin
makepkg -si
pacman -U hardcpy-bin-{Insert Version}-x86_64.pkg.tar.zst
```

Or you can use a wrapper such as [yay](https://github.com/Jguer/yay)
```sh
yay -S hardcpy-bin
```

## Compiling From Source

### AUR

Run the following commands
```sh
git clone https://aur.archlinux.org/hardcpy-git.git
cd hardcpy-git
makepkg -si
pacman -U hardcpy-git-{Insert Version}-x86_64.pkg.tar.zst
```

### Other distributions / Windows

Make sure you have [cargo](https://www.rust-lang.org/tools/install) installed

You should be getting something like the following
```sh
$ cargo --version
cargo 1.80.0 (376290515 2024-07-16)
```

Clone the repo and build the project
```sh
git clone https://github.com/obvMellow/hardcpy.git
cd hardcpy
cargo build --release
```

### Using Cargo

You can simply install it with the following command
```sh
cargo install hardcpy
```

And make sure you have ~/.cargo/bin in your $PATH
```sh
export PATH=$HOME/.cargo/bin:$PATH
```
