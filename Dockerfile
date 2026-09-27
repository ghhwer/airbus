# syntax=docker/dockerfile:1

# Statically linked musl binary with embedded dependency metadata (cargo auditable)
# → scratch runtime (no OS packages). Scan the binary with `cargo audit bin`.
FROM rust:1.86-bookworm AS builder
WORKDIR /src
RUN apt-get update \
	&& apt-get install -y --no-install-recommends musl-tools \
	&& rm -rf /var/lib/apt/lists/* \
	&& rustup target add x86_64-unknown-linux-musl \
	&& cargo install --locked cargo-auditable
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo auditable build --release --target x86_64-unknown-linux-musl

FROM scratch
COPY --from=builder /src/target/x86_64-unknown-linux-musl/release/airbus /airbus
COPY resources /opt/airbus/resources

ENV AIRBUS_LISTEN=0.0.0.0:9097 \
	AIRBUS_HTTP=0.0.0.0:9098 \
	AIRBUS_RESOURCES=/opt/airbus/resources/ui

EXPOSE 9097 9098

ENTRYPOINT ["/airbus"]
CMD ["--listen", "0.0.0.0:9097", "--http", "0.0.0.0:9098", "--resources", "/opt/airbus/resources/ui"]
