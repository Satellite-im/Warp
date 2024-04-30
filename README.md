# Warp

Interface Driven Distributed Data Service

### Overview

Warp can run as a single binary, providing an interface into the core technologies that run
Satellite. This allows us to avoid rewriting the same tech over and over when developing for
different platforms. Warp will work on most phones, tablets, computers, and consoles.

It provides abstractions to many different modules which are required to run Satellite. These
modules include Messaging, Caching, File Sharing & Storage, RTC connections, and more. Because we
focus on building these modules as interfaces first and then allow implementation layers to be built
on top of these, we can easily change the core technologies with no extra development required on
the "front-end" stacks. This means we can jump from multiple blockchains or decentralized solutions
without affecting the front-end application.

Additionally, libraries to interface with Warp (will) exist in JavaScript (TypeScript), Java,
Python, and more. So you can quickly develop your platforms and integrations on top of the Satellite
tech stack. Lastly, a REST API service can be enabled for Warp. However, it should never be exposed
outside of localhost.

### Build Requirement

#### Windows

***TBD***

#### Linux

**Ubuntu WSL (Maybe also Ubuntu + Debian)**
| Dep  | Install Command                                                  |
|------|------------------------------------------------------------------|
| Rust | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| Build Essentials | `sudo apt install build-essential` |
| CMake | `sudo apt install cmake` |
| LLVM libs & headers | `sudo apt install llvm-dev` |
| udev libs & headers | `sudo apt install libudev-dev` |

**Fedora 38**
| Dep  | Install Command                                                  |
|------|------------------------------------------------------------------|
| Rust | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| Build Essentials | `sudo dnf groupinstall "Development Tools" "Development Libraries"` |
| CMake | `sudo dnf install cmake` |
| LLVM libs & headers | `sudo dnf install llvm-devel` |
| udev libs & headers | `sudo dnf install libudev-devel` |

#### Mac

| Dep  | Install Command                                                  |
|------|------------------------------------------------------------------|
| Rust | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| CMake | `brew install cmake` |

## Building to WASM and using it within the browser

[See wasm docs](./tools/wasm-example/README.md)

## Docs

http://warp.satellite.im/
