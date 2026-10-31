# The web UI: static files, served by the server at /.
FROM node:24-bookworm-slim AS ui
RUN npm install -g pnpm@12.11.2
WORKDIR /src
# Manifests first so dependency installs cache across code changes.
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY apps/web/package.json apps/web/
COPY packages/api/package.json packages/api/
COPY crates/server/package.json crates/server/
RUN pnpm install --frozen-lockfile --filter @aircon/web...
COPY packages/api packages/api
COPY apps/web apps/web
RUN pnpm --filter @aircon/web exec vp build

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
COPY --from=ui /src/apps/web/dist /ui
COPY --from=build --chown=65532:65532 /data /data
ENV AIRCON_DB_PATH=/data/aircon.db \
  AIRCON_UI_DIR=/ui
VOLUME /data
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/aircon"]
