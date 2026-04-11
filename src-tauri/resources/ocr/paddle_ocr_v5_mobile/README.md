Place bundled OCR assets in this directory for packaging.

The bundled Paddle OCR v5 mobile ONNX assets are derived from the PP-OCRv5 models in the official PaddleOCR project:

- Source: https://github.com/PaddlePaddle/PaddleOCR
- License: Apache License 2.0
- License text: https://github.com/PaddlePaddle/PaddleOCR/blob/main/LICENSE

Required files:
- paddle_ocr_v5_mobile_text_detection.onnx
- paddle_ocr_v5_mobile_text_recognition.onnx
- paddle_ocr_v5_mobile_characters.txt

Optional files:
- text_line_orientation.onnx

At runtime, Flick resolves bundled resources from `ocr/paddle_ocr_v5_mobile/` first.
For local overrides during development, set `FLICK_OCR_PADDLE_OCR_V5_MOBILE_MODELS_DIR`.
