# Built from the repo root (see docker-compose.yml's `context: ../..`) since
# this is a workspace member and needs the workspace root Cargo.toml/lock.
# Not exercised in this dev environment (no Docker available here) - treat
# the first real build as the actual verification.
FROM rust:1-bookworm AS build
WORKDIR /workspace
COPY . .
RUN cargo build --release -p vibessh-backend

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /workspace/target/release/vibessh-backend /usr/local/bin/vibessh-backend
EXPOSE 8787
ENTRYPOINT ["/usr/local/bin/vibessh-backend"]
