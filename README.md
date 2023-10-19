## About The Project

![product-screenshot]

A simple editor for Firmware Attributes exposed with the Linux sysfs. </br>
These Attributes are not mandatory for Firmwares and can have different 
formats depends on vendor's implementation.

## Build

* Install rustup: https://rustup.rs/
* Build with `cargo build` or run with `cargo run`

## Usage

Access to sysfs requires a root access, so the Editor should be executed 
with the root privileges. </br>
If BIOS is protected by password, authentication will be requested on launch. 

[product-screenshot]: images/screenshot1.png