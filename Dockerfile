# Dockerfile concept from https://alexbrand.dev/post/how-to-package-rust-applications-into-minimal-docker-containers/
# and from https://dev.to/deciduously/use-multi-stage-docker-builds-for-statically-linked-rust-binaries-3jgd

FROM rust AS builder
WORKDIR /usr/src/frills

RUN rustup target add x86_64-unknown-linux-musl

RUN USER=root cargo init
COPY Cargo.toml .
COPY Cargo.lock .
RUN echo "fn main() {}" > src/frills_server.rs
RUN cargo build --target x86_64-unknown-linux-musl --release

COPY src ./src
RUN cargo build --target x86_64-unknown-linux-musl --release

FROM scratch
COPY --from=builder /usr/src/frills/target/x86_64-unknown-linux-musl/release/frills_server /bin/frills_server
EXPOSE 12345/tcp
CMD ["/bin/frills_server"]
