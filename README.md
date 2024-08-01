
[![Build hardcpy](https://github.com/obvMellow/hardcpy/actions/workflows/rust.yml/badge.svg)](https://github.com/obvMellow/hardcpy/actions/workflows/rust.yml)

# hardcpy
Simple backup tool written in Rust

# Installation

## Linux

### Arch Linux

On Arch Linux you can simply install it from the AUR
```sh
git clone https://aur.archlinux.org/hardcpy-git.git
cd hardcpy-git
makepkg -si
```

Or you can use a wrapper such as [yay](https://github.com/Jguer/yay)
```sh
yay -S hardcpy-git
```

### Other distributions

You can install it using cargo

Make sure you have [cargo](https://www.rust-lang.org/tools/install) installed

You should be getting something like the following
```sh
$ cargo --version
cargo 1.80.0 (376290515 2024-07-16)
```

Then you can simply install it with the following command
```sh
cargo install hardcpy
```
