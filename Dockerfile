# syntax=docker/dockerfile:1.0.0-experimental
FROM rust AS planner
WORKDIR /usr/src/frills
RUN rustup default nightly
RUN cargo install cargo-chef
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM rust AS cacher
WORKDIR /usr/src/frills
RUN rustup default nightly
RUN rustup target add x86_64-unknown-linux-musl
RUN cargo install cargo-chef
COPY --from=planner /usr/src/frills/recipe.json recipe.json
RUN cargo chef cook --target x86_64-unknown-linux-musl --release --recipe-path recipe.json

FROM rust AS builder
WORKDIR /usr/src/frills
RUN rustup default nightly
RUN rustup target add x86_64-unknown-linux-musl
COPY . .
COPY --from=cacher /usr/src/frills/target target
COPY --from=cacher $CARGO_HOME $CARGO_HOME
RUN cargo build --target x86_64-unknown-linux-musl --release

FROM scratch
COPY --from=builder /usr/src/frills/target/x86_64-unknown-linux-musl/release/frills_server /bin/frills_server
EXPOSE 12345/tcp
CMD ["/bin/frills_server"]