#!/bin/sh
# Download the Supertonic-2 ONNX models + voice styles.
# These are omitted from the repo (~260 MB) — this script fetches them from HF.
#
# Output directory is $ASSETS_DIR, defaulting to ./assets next to the script's repo.
set -eu

BASE="https://huggingface.co/Supertone/supertonic-2/resolve/main"
if [ -z "${ASSETS_DIR:-}" ]; then
  ASSETS_DIR="$(cd "$(dirname "$0")/.." && pwd)/assets"
fi

mkdir -p "$ASSETS_DIR/onnx" "$ASSETS_DIR/voice_styles"

fetch() {
  if [ -f "$ASSETS_DIR/$1" ]; then
    echo "skip $1"
  else
    echo "get  $1"
    curl -sSL --fail -o "$ASSETS_DIR/$1" "$BASE/$1"
  fi
}

fetch config.json
for f in duration_predictor.onnx text_encoder.onnx tts.json unicode_indexer.json vector_estimator.onnx vocoder.onnx; do
  fetch "onnx/$f"
done
for v in F1 F2 F3 F4 F5 M1 M2 M3 M4 M5; do
  fetch "voice_styles/$v.json"
done

echo "done — $(du -sh "$ASSETS_DIR" | cut -f1) in $ASSETS_DIR"
