CLIENT_DIR := client
TARGET_DIR := out
TARGET := $(TARGET_DIR)/airbus
SCHEMA_DIR := schema/payloads

.PHONY: all clean run setup client test test-unit test-integration generate build

all: build

build:
	mkdir -p $(TARGET_DIR)
	cargo build --release
	cp -f target/release/airbus $(TARGET)

run: build
	./$(TARGET) --listen 127.0.0.1:9097

# Prefer the repo-root uv workspace (editable airbus-client + shared .venv).
setup:
	cd .. && uv sync --package airbus-client --all-groups

client: build setup
	cd $(CLIENT_DIR) && AIRBUS_BIN="$(CURDIR)/$(TARGET)" uv run python -m airbus_client

# Regenerate Rust + Python payload types from schema/payloads/*.schema.json
# Requires: cargo-typify (cargo install cargo-typify) and client dev deps (make setup).
generate: setup
	python3 scripts/generate_payloads.py

test-unit:
	cargo test

test-integration: build setup
	cd $(CLIENT_DIR) && AIRBUS_BIN="$(CURDIR)/$(TARGET)" uv run pytest

test: test-unit test-integration

clean:
	cargo clean
	rm -f $(TARGET)
	rm -f $(SCHEMA_DIR)/bundle.schema.json $(SCHEMA_DIR)/python_bundle.schema.json
