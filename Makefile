CLIENT_DIR := client
TARGET_DIR := out
TARGET := $(TARGET_DIR)/airbus
SCHEMA_DIR := schema/payloads
HTTP_URL ?= 127.0.0.1:9098
RESOURCES ?= $(CURDIR)/resources/ui

.PHONY: all clean run setup client test test-unit test-integration generate build check-protocol

all: build

build:
	mkdir -p $(TARGET_DIR)
	cargo build --release
	@if [ -n "$$CARGO_TARGET_DIR" ] && [ -f "$$CARGO_TARGET_DIR/release/airbus" ]; then \
		cp -f "$$CARGO_TARGET_DIR/release/airbus" $(TARGET); \
	elif [ -f "target/release/airbus" ]; then \
		cp -f target/release/airbus $(TARGET); \
	else \
		cp -f $$(cargo metadata --format-version 1 2>/dev/null | jq -r .target_directory 2>/dev/null)/release/airbus $(TARGET); \
	fi

run: build
	./$(TARGET) --listen 127.0.0.1:9097 --http $(HTTP_URL) --resources $(RESOURCES)

# Prefer the repo-root uv workspace (editable airbus-client + shared .venv).
setup:
	cd .. && uv sync --all-packages

client: build setup
	cd $(CLIENT_DIR) && AIRBUS_BIN="$(CURDIR)/$(TARGET)" uv run python -m airbus_client

# Regenerate Rust + Python payload types from schema/payloads/*.schema.json
# Requires: cargo-typify (cargo install cargo-typify) and client dev deps (make setup).
generate: setup
	uv run python scripts/generate_payloads.py

.PHONY: check-protocol
check-protocol:
	uv run python scripts/check_protocol_boundaries.py
	uv run python scripts/generate_payloads.py --check

test-unit:
	cargo test
	node tests/protocol.test.mjs

test-integration: build setup
	cd $(CLIENT_DIR) && AIRBUS_BIN="$(CURDIR)/$(TARGET)" uv run pytest

test: check-protocol test-unit test-integration

clean:
	cargo clean
	rm -f $(TARGET)
	rm -f $(SCHEMA_DIR)/bundle.schema.json $(SCHEMA_DIR)/python_bundle.schema.json
