FROM rustlang/rust:nightly as builder

ENV DEBIAN_FRONTEND=noninteractive

RUN apt update && apt install -y git bash make gcc linux-libc-dev patch musl musl-tools musl-dev

RUN rustup target add x86_64-unknown-linux-musl

COPY . /rumble-build

WORKDIR /rumble-build

RUN --mount=type=cache,target=/usr/local/cargo,from=rust,source=/usr/local/cargo \
    --mount=type=cache,target=target \
    cargo build --release --target x86_64-unknown-linux-musl && cp target/x86_64-unknown-linux-musl/release/rust-mumble /rust-mumble

FROM scratch

COPY --from=builder /rust-mumble /rust-mumble

EXPOSE 64738/udp
EXPOSE 64738/tcp
EXPOSE 8080/tcp

ENV RUST_LOG=info

CMD ["/rust-mumble"] # Password should be passed via args
