SHELL := /bin/bash

BUNDLE_DIR := target/release/bundle
APP_DIR := $(BUNDLE_DIR)/macos
DMG_DIR := $(BUNDLE_DIR)/dmg
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
	@DMG_PATH="$$(find "$(DMG_DIR)" -maxdepth 1 -name '*.dmg' | head -n 1)"; \
	if [ -n "$$DMG_PATH" ]; then \
		open "$$DMG_PATH"; \
	else \
		echo "No .dmg found under $(DMG_DIR). Run 'make build-release' first."; \
		exit 1; \
	fi

clean:
	cd src-tauri && cargo clean
