# Zumble

A mumble server for FiveM.

Goal is to have an external server handling voice chat for FiveM servers instead of using the built-in voice chat.

**This is a work in progress. Use it at your own risk.**

## Features
 * 100% compatible with FiveM -> drop in replacement for client natives
 * Http api to mimic server side native calls (MumbleIsPlayerMuted / MumbleSetPlayerMuted / MumbleCreateChannel)
 * Performance (multithreaded server, separated from game network)
 * Can be installed on a separate machine
 * prometheus metrics

## Installation

 1. Clone this repository
 2. If you're on Linux your system needs to have `llvm` and `make` installed
 3. Build the server using cargo: `cargo build --release`

Future versions will include pre-built binaries in release section of GitHub.

## Usage

NOTE: This might be out of date, you can run the binary with `--help` to view the most up to date version.

FiveM servers will want to run with `rust-mumble --restrict-to-version CitizenFX` to help prevent external clients from joining

```
Usage: rust-mumble [OPTIONS]

Options:
      --help
          
  -l, --listen <LISTEN>
          Listen address for TCP and UDP connections for mumble voip clients (or other clients that support the mumble protocol) [default: 0.0.0.0:64738]
  -h, --http-listen <HTTP_LISTEN>
          Listen address for HTTP connections for the admin api [default: 0.0.0.0:8080]
      --http-user <HTTP_USER>
          User for the http server api basic authentification [default: admin]
      --http-password <HTTP_PASSWORD>
          Password for the http server api basic authentification
      --https
          Use TLS for the http server (https), will use the same certificate as the mumble server
      --http-log
          Log http requests to stdout
      --key <KEY>
          Path to the key file for the TLS certificate [default: key.pem]
      --cert <CERT>
          Path to the certificate file for the TLS certificate [default: cert.pem]
  -r, --restrict-to-version <RESTRICT_TO_VERSION>
          Restricts the clients release name to a specific version, useful for providing a stop-gap if you want to stop external clients from joining, setting to `CitizenFX` will only allow clients with the CitizenFX mumble client to join
  -V, --version
          Print version
```

## Credits

  * [mumble-protocol](https://github.com/Johni0702/rust-mumble-protocol) for the crypt / decrypt algorithm of the mumble protocol, it was rewritten here to work on pure rust library (no openssl)
