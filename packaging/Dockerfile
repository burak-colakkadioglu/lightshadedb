FROM rust:1.97.0-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY src ./src
COPY codegen ./codegen
COPY macros ./macros
COPY examples ./examples
RUN cargo build --release --bin lightshade
RUN ./target/release/lightshade check ./examples/messages.lightshade
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /srv/lightshade
COPY --from=build /src/target/release/lightshade /usr/local/bin/lightshade
COPY --from=build /src/examples/messages.lightshade /srv/lightshade/messages.lightshade
COPY packaging/lightshade.env.example /srv/lightshade/messages.env.example
ENV LIGHTSHADE_CREDENTIALS=/etc/lightshade/messages.credentials
EXPOSE 7171
VOLUME ["/srv/lightshade"]
ENTRYPOINT ["/usr/local/bin/lightshade"]
CMD ["serve", "/srv/lightshade/messages.lightshade", "/srv/lightshade/messages.lsdb"]
