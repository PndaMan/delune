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
    cargo build --release --locked -p delune && cp target/release/delune /delune

# ---- Runtime ----------------------------------------------------------------
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /delune /usr/local/bin/delune
ENV DELUNE_BIND=0.0.0.0:7474
EXPOSE 7474
# 2234 will be the Soulseek listening port once the client is connected.
EXPOSE 2234
ENTRYPOINT ["delune"]
CMD ["serve"]
