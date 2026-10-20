# syntax=docker/dockerfile:1

FROM node:24.21.0-bookworm-slim AS web
WORKDIR /web
COPY exit-withdraw/claim-web/package.json exit-withdraw/claim-web/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci
COPY exit-withdraw/claim-web/ ./
RUN npm run build

FROM rust:1.95.0-bookworm AS cli
RUN apt-get update \
    && apt-get install -y --no-install-recommends clang libclang-dev cmake pkg-config \
    && rm -rf /var/lib/apt/lists/*
ARG TARGETARCH
ARG CARGO_BUILD_JOBS=4
ENV CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS}
WORKDIR /src
COPY exit-withdraw/ ./exit-withdraw/
COPY gwos/crates/ ./gwos/crates/
COPY gwos/contracts/exit-withdraw-state-validator/ ./gwos/contracts/exit-withdraw-state-validator/
COPY crates/smt/ ./crates/smt/
COPY crates/store/ ./crates/store/
WORKDIR /src/exit-withdraw
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,id=godwoken-refund-target-${TARGETARCH},target=/src/exit-withdraw/target \
    cargo build --locked --release --bin gw-withdraw \
    && install -D target/release/gw-withdraw /out/gw-withdraw \
    && strip /out/gw-withdraw

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       ca-certificates libstdc++6 zlib1g libbz2-1.0 liblz4-1 libzstd1 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --user-group --no-create-home --shell /usr/sbin/nologin wallet
COPY --from=cli /out/gw-withdraw /usr/local/bin/gw-withdraw
COPY --from=web /web/dist/ /app/web/
RUN gw-withdraw wallet-serve --help > /dev/null
WORKDIR /app
USER wallet:wallet
EXPOSE 8787
STOPSIGNAL SIGINT
ENTRYPOINT ["/usr/local/bin/gw-withdraw", "wallet-serve", "--web-dir", "/app/web", "--listen", "0.0.0.0:8787"]
CMD ["--public-origin", "http://127.0.0.1:8787", "--enable-broadcast"]
