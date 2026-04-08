Place bundled OCR assets in this directory for packaging.

Required files:
- paddle_ocr_v5_mobile_text_detection.onnx
- paddle_ocr_v5_mobile_text_recognition.onnx
- paddle_ocr_v5_mobile_characters.txt

Optional files:
- text_line_orientation.onnx

At runtime, Flick resolves bundled resources from `ocr/paddle_ocr_v5_mobile/` first.
For local overrides during development, set `FLICK_OCR_PADDLE_OCR_V5_MOBILE_MODELS_DIR`.
