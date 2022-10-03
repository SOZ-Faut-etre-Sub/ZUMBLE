FROM rust:1.63.0 as builder

ENV DEBIAN_FRONTEND=noninteractive

RUN apt update && apt install -y git bash make gcc linux-libc-dev patch musl musl-tools musl-dev openssl

RUN rustup target add x86_64-unknown-linux-musl

COPY . /zumble-build

WORKDIR /zumble-build

RUN openssl req -newkey rsa:2048 -new -nodes -x509 -days 3650 -keyout /key.pem -out /cert.pem -subj "/C=FR/ST=Paris/L=Paris/O=SoZ/CN=soz.zerator.com"

RUN --mount=type=cache,target=/usr/local/cargo,from=rust,source=/usr/local/cargo \
    --mount=type=cache,target=target \
    cargo build --release --target x86_64-unknown-linux-musl && cp target/x86_64-unknown-linux-musl/release/zumble /zumble

FROM scratch

COPY --from=builder /zumble /zumble
COPY --from=builder /cert.pem /cert.pem
COPY --from=builder /key.pem /key.pem

EXPOSE 64738/udp
EXPOSE 64738/tcp
EXPOSE 8080/tcp

ENV RUST_LOG=info

CMD ["/zumble", "--http-password", "changeme"]
