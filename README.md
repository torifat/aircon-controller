# aircon-controller

Throttles a ducted Daikin behind an AirTouch 5 so rooms hold temperature without the spill
zone freezing. The AirTouch still owns zones (on/off, setpoints, dampers); this controller
drives the AC itself from the zone sensors:

- **Fan** steps Low / Medium / High with the gap between the worst active room and its
  target (1° and 2° by default, with a 0.2° margin before stepping down).
- **AC setpoint** is pulled below (cooling) or above (heating) the AC's own reading by that
  gap, so the inverter works hard when rooms are far off and idles near target.
- **Off at target**: when every active room reaches target the AC is turned off, and back on
  once one drifts 2° past it. Min 5 min on / 5 min off protects the compressor.
- **Hands off**: an AC switched off by someone else is never turned back on; a fan, setpoint
  or mode change made in the AirTouch app pauses the controller for 30 min. Zone changes
  don't pause: they're how you steer it.
- Rooms in **percentage control** have no usable AirTouch setpoint, so their target is kept
  here (set from the UI).

Starts in **dry run**: decisions are logged ("Would: …") but nothing is sent.

## Layout

A Cargo workspace plus a pnpm workspace driven by Vite+ (`vp`), like pass-extension.

```
crates/server/   the service: AirTouch protocol + connection, control logic, SQLite, HTTP API
packages/api/    API types for TypeScript, generated from crates/server by ts-rs
apps/web/        the UI (Svelte 5, Tailwind v4), built into the image and served at /
```

| File in `crates/server/src` |                                                                           |
| --------------------------- | ------------------------------------------------------------------------- |
| `protocol.rs`               | AirTouch 5 wire format; tests use bytes captured from the console         |
| `airtouch.rs`               | TCP session, reconnects, keepalive; `Device` trait (faked in tests)       |
| `logic.rs`                  | `decide()`: pure function from snapshot + settings + memory to a decision |
| `controller.rs`             | the loop: tick, send, log activity, record readings                       |
| `store.rs`                  | SQLite: readings, activity, settings, runtime                             |
| `api.rs`                    | axum routes and the API types                                             |

## Commands

Toolchain: mise (`mise.toml`: Node 24, pnpm, and Rust from `rust-toolchain.toml`). Outside an
activated mise shell, prefix with `mise exec --`.

```sh
pnpm install
vp run ready                                   # format/lint, clippy, tsc, tests, build
cargo test                                     # just the Rust tests
vp run @aircon/api#generate                    # regenerate TS types after changing API structs

# UI dev server, proxying /api to a local `cargo run` (or set AIRCON_API to another controller)
cd apps/web && vp dev

AIRCON_AIRTOUCH_HOST=<console ip> cargo run   # http://localhost:8080
curl -s localhost:8080/api/state | jq .controller
curl -s -X PATCH localhost:8080/api/settings -H 'content-type: application/json' \
  -d '{"dry_run": false}'                      # go live
```

| Env                    | Default                                           |                                     |
| ---------------------- | ------------------------------------------------- | ----------------------------------- |
| `AIRCON_AIRTOUCH_HOST` | —                                                 | AirTouch 5 console IP (required)    |
| `AIRCON_DB_PATH`       | `data/aircon.db` (`/data/aircon.db` in the image) | SQLite: history, activity, settings |
| `AIRCON_UI_DIR`        | —                                                 | built UI served at `/`              |
| `AIRCON_LISTEN`        | `0.0.0.0:8080`                                    |                                     |
| `RUST_LOG`             | `info`                                            |                                     |

Everything else (dry run, deadband, fan thresholds, timings, which rooms count) is a setting,
changed through the API and kept in the database.

## API

|                                          |                                                                     |
| ---------------------------------------- | ------------------------------------------------------------------- |
| `GET /api/state`                         | AC, zones (with target, gap, driving), controller status and reason |
| `POST /api/tick`                         | run a check now; returns state                                      |
| `GET /api/history?hours=24`              | per-zone temperature/target/on and AC on/fan/setpoint, bucketed     |
| `GET /api/activity?kind=&before=&limit=` | decisions, info, external changes, errors                           |
| `GET` / `PATCH /api/settings`            | tunables                                                            |
| `PATCH /api/zones/{n}`                   | `{"on": true, "target": 23.5}`                                      |
| `PUT /api/ac`                            | `{"on": false}`                                                     |
| `POST /api/pause` / `POST /api/resume`   | `{"minutes": 30}`                                                   |
| `GET /healthz`, `GET /readyz`            | liveness; readiness = connected to AirTouch                         |

## Release

GitHub Actions runs fmt, clippy, tests and a generated-types drift check on every push, and on
`main` and `v*` tags publishes `ghcr.io/torifat/aircon-controller` (`edge`, `sha-…`, and
`X.Y.Z` for tags):

```sh
git tag v0.1.0 && git push origin v0.1.0
```
