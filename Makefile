SHELL := /bin/bash

BUNDLE_DIR := target/release/bundle
APP_DIR := $(BUNDLE_DIR)/macos
DMG_DIR := $(BUNDLE_DIR)/dmg
TAURI_CLI := ./frontend/node_modules/.bin/tauri

.PHONY: release build-release open-release clean
.PHONY: check-linux-deps setup-linux-deps-ubuntu

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
		if command -v open >/dev/null 2>&1; then \
			open "$$DMG_PATH"; \
		elif command -v xdg-open >/dev/null 2>&1; then \
			xdg-open "$$DMG_PATH"; \
		else \
			echo "$$DMG_PATH"; \
		fi; \
	else \
		echo "No .dmg found under $(DMG_DIR). Run 'make build-release' first."; \
		exit 1; \
	fi

check-linux-deps:
	@missing=0; \
	for pkg in glib-2.0 gtk+-3.0 webkit2gtk-4.1 ayatana-appindicator3-0.1 librsvg-2.0 xdo openssl; do \
		if ! pkg-config --exists "$$pkg"; then \
			echo "Missing pkg-config package: $$pkg"; \
			missing=1; \
		fi; \
	done; \
	if [ "$$missing" -eq 0 ]; then \
		echo "Linux desktop build dependencies look installed."; \
	else \
		echo "Install them with: make setup-linux-deps-ubuntu"; \
		exit 1; \
	fi

setup-linux-deps-ubuntu:
	sudo apt-get update
	sudo apt-get install -y \
		build-essential \
		curl \
		wget \
		file \
		pkg-config \
		libglib2.0-dev \
		libgtk-3-dev \
		libwebkit2gtk-4.1-dev \
		libayatana-appindicator3-dev \
		librsvg2-dev \
		libxdo-dev \
		libssl-dev

clean:
	cd src-tauri && cargo clean
