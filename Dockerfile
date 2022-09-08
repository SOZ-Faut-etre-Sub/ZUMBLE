FROM rust:1.63.0 as builder

ENV DEBIAN_FRONTEND=noninteractive

RUN apt update && apt install -y git bash make gcc linux-libc-dev patch musl musl-tools musl-dev

RUN rustup target add x86_64-unknown-linux-musl

COPY . /zumble-build

WORKDIR /zumble-build

RUN --mount=type=cache,target=/usr/local/cargo,from=rust,source=/usr/local/cargo \
    --mount=type=cache,target=target \
    cargo build --release --target x86_64-unknown-linux-musl && cp target/x86_64-unknown-linux-musl/release/zumble /zumble

FROM scratch

COPY --from=builder /zumble /zumble
COPY cert.pem /cert.pem
COPY key.pem /key.pem

EXPOSE 64738/udp
EXPOSE 64738/tcp
EXPOSE 8080/tcp

ENV RUST_LOG=info

CMD ["/zumble", "--http-password", "changeme"]
