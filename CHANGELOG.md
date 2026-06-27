# Changelog

All notable changes to Flick will be documented in this file.

## [1.1.3] - 2026-06-27

### Improved

- Improved screenshot editor text annotation editing so draft text matches final rendered size and uses a transparent inline input without a visible box.
- Improved screenshot editor annotation selection so switching tools clears selection handles, while selecting an existing annotation synchronizes the active tool, color, and size controls.
- Improved screenshot editor toolbar adjustments so color, line width, and text size changes apply to the currently selected annotation when supported.

## [1.1.2] - 2026-06-27

### Fixed

- Improved ffmpeg detection on macOS installed builds by checking Homebrew's common install paths, `/usr/local/bin/ffmpeg` and `/opt/homebrew/bin/ffmpeg`, before falling back to `PATH` lookup.
- Fixed installed macOS app builds failing to detect ffmpeg when launched from Finder, Dock, or login items with a GUI environment that does not include Homebrew directories in `PATH`.

## [1.1.1] - 2026-06-27

### Added

- Added pinned image support, including UI and backend integration.
- Added a Number Tag annotation tool to the screenshot editor.
- Added selection frame visualization and preview support for long captures in the screenshot editor.
- Improved overlay behavior with crosshair support and monitor scaling refinements.

### Improved

- Refined long-capture scrolling, near-duplicate detection, and Windows scroll responsiveness.
- Improved toolbar sizing and pin-to-desktop behavior.
- Removed verbose diagnostic logging from screenshot editor and scroll controller paths.

## [1.1.0] - 2026-06-25

### Added

- Added long capture support with toolbar-based scroll controls, improved stitching, and platform-specific scrolling integrations.
- Added GIF recording with localized toolbar controls and native tooltips.
- Added MP4 video recording support alongside GIF recording.
- Added cross-platform system audio capture, including Windows WASAPI loopback support.
- Added ffmpeg download progress reporting and video recording availability checks with fallback to GIF.
- Added video thumbnail generation and paginated capture history.
- Added localized screenshot editor support.

### Improved

- Modularized screenshot editor components and recording window behavior.
- Improved screenshot editor toolbar positioning and interaction.
- Improved recording frame handling, format handling, and capture workflow stability.
- Improved Windows file opening through `ShellExecuteW`.
- Updated README documentation for screenshot editor features, platform support, and Linux limitations.
- Removed verbose diagnostic logging from capture, recording, and audio paths.

## [1.0.0] - 2026-04-11

### Highlights

- Initial open-source release of Flick.
- Added desktop screenshot capture with configurable global hotkeys.
- Added screenshot-to-translation workflow with OCR and AI translation.
- Added selected-text translation and translate-and-replace shortcuts.
- Added platform-aware OCR support for macOS Vision OCR, Windows built-in OCR, and bundled Paddle OCR v5 mobile ONNX models.
- Added AI translation provider support for OpenAI, Anthropic, OpenAI-compatible endpoints, Anthropic-compatible endpoints, Ollama, and LM Studio.
- Added Microsoft Edge TTS playback for source and translated text where available.
- Added local screenshot history and translation history.
- Added configurable screenshot retention and screenshot storage path.
- Added launch-at-startup support.
- Added interface language support for English, Simplified Chinese, and Japanese.
- Documented Linux Wayland limitations for global hotkeys and screenshot capture.
- Documented bundled PaddleOCR asset source and Apache License 2.0 attribution.
