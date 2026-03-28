SHELL := /bin/bash

APP_DIR := src-tauri/target/release/bundle/macos
TAURI_CLI := ./frontend/node_modules/.bin/tauri

.PHONY: release build-release open-release clean

release: build-release open-release

build-release:
	@if [ ! -x "$(TAURI_CLI)" ]; then \
		echo "Missing local Tauri CLI at $(TAURI_CLI). Run 'cd frontend && npm install' first."; \
		exit 1; \
	fi
	$(TAURI_CLI) build --config src-tauri/tauri.conf.json

open-release:
	@APP_PATH="$$(find "$(APP_DIR)" -maxdepth 1 -name '*.app' | head -n 1)"; \
	if [ -z "$$APP_PATH" ]; then \
		echo "No .app found under $(APP_DIR). Run 'make build-release' first."; \
		exit 1; \
	fi; \
	open "$$APP_PATH"

clean:
	cd src-tauri && cargo clean
