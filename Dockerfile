FROM rust:1.96.0-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY .cargo .cargo
COPY crates crates
RUN --mount=type=cache,target=/usr/local/cargo/registry \
  --mount=type=cache,target=/src/target \
  cargo build --release --locked -p aircon-server \
  && cp target/release/aircon /aircon \
  && mkdir /data

# glibc + libgcc only; SQLite is compiled into the binary.
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /aircon /usr/local/bin/aircon
COPY --from=build --chown=65532:65532 /data /data
ENV AIRCON_DB_PATH=/data/aircon.db
VOLUME /data
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/aircon"]
