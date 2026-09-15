# syntax=docker/dockerfile:1

# ---- Web UI -----------------------------------------------------------------
FROM oven/bun:1 AS web
WORKDIR /src/web
COPY web/package.json web/bun.lock ./
RUN bun install --frozen-lockfile
COPY web/ ./
RUN bun run build

# ---- Binary -----------------------------------------------------------------
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
COPY --from=web /src/web/dist web/dist
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    # Two jobs keep memory in check on small build machines; thin LTO is memory hungry.
    CARGO_BUILD_JOBS=2 cargo build --release --locked -p delune && cp target/release/delune /delune \
    && mkdir /data

# ---- Runtime ----------------------------------------------------------------
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /delune /usr/local/bin/delune
# The data volume must belong to the unprivileged user the image runs as.
COPY --from=build --chown=nonroot:nonroot /data /data
ENV DELUNE_BIND=0.0.0.0:7474
EXPOSE 7474
# Soulseek peers connect here.
EXPOSE 2234
VOLUME ["/data"]
ENV DELUNE_DATA_DIR=/data
ENTRYPOINT ["delune"]
CMD ["serve"]
