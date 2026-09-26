CLIENT_DIR := client-py
CLIENT_CPP_DIR := client-cpp
TARGET_DIR := out
TARGET := $(TARGET_DIR)/airbus
SCHEMA_DIR := schema/payloads
LISTEN ?= 0.0.0.0:9097
HTTP_URL ?= 0.0.0.0:9098
RESOURCES ?= $(CURDIR)/resources/ui

.PHONY: all clean run setup client test test-unit test-integration test-client-cpp \
	generate build check-protocol docker docker-run publish-client pack-client-cpp help

all: build

help:
	@echo "Airbus targets:"
	@echo "  make build              cargo build --release → out/airbus"
	@echo "  make run                TCP + HTTP debug UI"
	@echo "  make setup              uv sync (workspace + client-py)"
	@echo "  make test               protocol checks + unit + py/cpp integration"
	@echo "  make test-client-cpp    build + run C++ client Catch2 tests"
	@echo "  make generate           regenerate Rust/Python/C++ payloads from schema"
	@echo "  make docker             build container image airbus:local"
	@echo "  make docker-run         run container (9097 TCP, 9098 HTTP)"
	@echo "  make publish-client     build+publish airbus-client wheel (needs UV_PUBLISH_*)"
	@echo "  make pack-client-cpp    zip C++ + PlatformIO release assets → dist/"

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
	./$(TARGET) --listen $(LISTEN) --http $(HTTP_URL) --resources $(RESOURCES)

setup:
	uv sync --all-packages

client: build setup
	cd $(CLIENT_DIR) && AIRBUS_BIN="$(CURDIR)/$(TARGET)" uv run python -m airbus_client

generate: setup
	uv run python scripts/generate_payloads.py

check-protocol:
	uv run python scripts/check_protocol_boundaries.py
	uv run python scripts/generate_payloads.py --check

test-unit:
	cargo test
	node tests/protocol.test.mjs

test-integration: build setup
	cd $(CLIENT_DIR) && AIRBUS_BIN="$(CURDIR)/$(TARGET)" uv run pytest

test-client-cpp: build
	cmake -S $(CLIENT_CPP_DIR) -B $(CLIENT_CPP_DIR)/build \
		-DAIRBUS_CLIENT_BUILD_EXAMPLES=OFF \
		-DAIRBUS_CLIENT_BUILD_TESTS=ON
	cmake --build $(CLIENT_CPP_DIR)/build -j$$(nproc)
	cd $(CLIENT_CPP_DIR)/build && AIRBUS_BIN="$(CURDIR)/$(TARGET)" ctest --output-on-failure

test: check-protocol test-unit test-integration test-client-cpp

docker:
	docker build -t airbus:local .

docker-run:
	docker run --rm -p 9097:9097 -p 9098:9098 airbus:local

publish-client: setup
	cd $(CLIENT_DIR) && uv build && uv publish

pack-client-cpp:
	python3 scripts/pack_cpp_clients.py --out-dir dist

clean:
	cargo clean
	rm -f $(TARGET)
	rm -rf dist
	rm -rf $(CLIENT_CPP_DIR)/build
	rm -f $(SCHEMA_DIR)/bundle.schema.json $(SCHEMA_DIR)/python_bundle.schema.json
