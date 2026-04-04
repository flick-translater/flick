Place bundled OCR assets in this directory for packaging.

Required files:
- text_detection.onnx
- text_recognition.onnx
- characters.txt

Optional files:
- text_line_orientation.onnx

At runtime, Flick resolves bundled resources from `ocr/onnx/` first.
For local overrides during development, set `FLICK_OCR_ONNX_MODELS_DIR`.
