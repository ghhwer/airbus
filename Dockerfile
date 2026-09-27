# syntax=docker/dockerfile:1

FROM rust:1.85-bookworm AS builder
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
	&& apt-get upgrade -y --no-install-recommends \
	&& apt-get install -y --no-install-recommends ca-certificates \
	&& rm -rf /var/lib/apt/lists/*

COPY --from=builder /src/target/release/airbus /usr/local/bin/airbus
COPY resources /opt/airbus/resources

ENV AIRBUS_LISTEN=0.0.0.0:9097 \
	AIRBUS_HTTP=0.0.0.0:9098 \
	AIRBUS_RESOURCES=/opt/airbus/resources/ui

EXPOSE 9097 9098

ENTRYPOINT ["airbus"]
CMD ["--listen", "0.0.0.0:9097", "--http", "0.0.0.0:9098", "--resources", "/opt/airbus/resources/ui"]
