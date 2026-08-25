FROM mcr.microsoft.com/devcontainers/rust:1-bookworm

# Native build deps NexQ pulls in:
#   protobuf-compiler     -> KEDA external-scaler gRPC interface (plan Q16)
#   pkg-config/libssl-dev -> TLS for the HTTP facades + backend clients
#   libsqlite3-dev        -> sqlite backend
#   postgresql-client     -> psql for poking the postgres backend
#   musl-tools            -> static builds for the small runtime image (plan Q21)
RUN apt-get update \
    && export DEBIAN_FRONTEND=noninteractive \
    && apt-get install -y --no-install-recommends \
        protobuf-compiler \
        pkg-config \
        libssl-dev \
        libsqlite3-dev \
        sqlite3 \
        postgresql-client \
        musl-tools \
        jq \
        unzip \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

USER vscode

RUN rustup component add rustfmt clippy rust-src rust-analyzer \
    && rustup target add x86_64-unknown-linux-musl

# cargo-binstall fetches prebuilt binaries instead of compiling each tool from
# source, keeping the image build to minutes rather than tens of minutes.
RUN curl -L --proto '=https' --tlsv1.2 -sSf \
        https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash \
    && cargo binstall -y --no-symlinks cargo-nextest cargo-watch cargo-deny

USER root
