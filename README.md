# wyoming-supertonic

> A [Wyoming protocol](https://github.com/rhasspy/wyoming) text-to-speech server backed by [Supertonic-2](https://huggingface.co/Supertone/supertonic-2) — natural-sounding TTS with great performance.

Drop-in replacement for [Piper](https://github.com/rhasspy/wyoming-piper) inside [Home Assistant voice pipelines](https://www.home-assistant.io/voice_control/) (and anything else that speaks Wyoming). Runs entirely on your own hardware — no cloud, no API keys.

## Use cases

**Home Assistant voice reply**: Plug it into your Assist pipeline and you've got a local voice assistant that *sounds like a person*, not a robot announcing a train station.

**Smart-home notifications that don't feel jarring**: "The dryer is finished" or "Your delivery is downstairs" in a natural voice is much easier to live with than Piper's charmingly-mechanical cadence.

**Accessible on-device reader**: Turn webpages or emails into audio on a Raspberry Pi or a cheap mini-PC — no network dependency, works offline.

## Voices

Ten voices are bundled — five female (F1–F5) and five male (M1–M5). You can listen to all of them side-by-side in [this Supertonic 2 voice comparison](https://adamjones.me/blog/supertonic-v2-voice-comparison).

I think **F2** (Female 2) is the best and is the default if you don't choose one.

## Compared to Piper

|   | Supertonic | Piper |
|---|---|---|
| **Voice quality** | 🏆 generally higher¹ ([listen](https://adamjones.me/blog/supertonic-v2-voice-comparison)) | generally lower ([listen](https://rhasspy.github.io/piper-samples/)) |
| **Generation speed** | 🏆 slightly faster ([numbers](#performance)) | slightly slower |
| **Memory usage** | 🟰 ~300 MB | 🟰 ~300 MB |
| **Privacy** | 🟰 fully local | 🟰 fully local |
| **Multilingual support** | English-focused | 🏆 broader multilingual support |

¹: More natural, expressive, smoother, and better at handling abbreviations like "$10M", "5 km" or "Fri 17 Apr, 2026."

## Quick start

### Docker

```bash
docker run -p 10220:10220 ghcr.io/domdomegg/wyoming-supertonic
```

### Kubernetes

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: wyoming-supertonic
spec:
  replicas: 1
  selector:
    matchLabels:
      app: wyoming-supertonic
  template:
    metadata:
      labels:
        app: wyoming-supertonic
    spec:
      containers:
        - name: wyoming-supertonic
          image: ghcr.io/domdomegg/wyoming-supertonic:latest
          ports:
            - containerPort: 10220
---
apiVersion: v1
kind: Service
metadata:
  name: wyoming-supertonic
spec:
  selector:
    app: wyoming-supertonic
  ports:
    - port: 10220
      targetPort: 10220
```

## Home Assistant setup

1. **Settings → Devices & Services → Add Integration → Wyoming Protocol**.
2. **Host**: your server's hostname or IP. **Port**: `10220` (or whatever you mapped).
3. Go to **Settings → Voice Assistants**, pick or create a pipeline, and set **Text-to-speech** to `supertonic`. Pick a voice from the dropdown (Female 1, Male 2, …).

## Configuration

All configuration is via environment variables. Defaults are sensible.

| Variable | Default | Description |
|---|---|---|
| `PORT` | `10220` | TCP port to listen on |
| `SUPERTONIC_DEFAULT_VOICE` | `F2` | Voice used when a client doesn't specify one. One of `F1`–`F5` (female) or `M1`–`M5` (male). |
| `TOTAL_STEPS` | `3` | Diffusion denoising steps. Higher = better quality, slower. Range 1–20. |
| `TRANSFORM_SPEED` | `1.4` | Speaking rate, applied as a pitch-preserving time-stretch to the rendered audio. `1.0` is natural, `1.4` is a snappy assistant pace, `1.8` is fast. **Prefer this knob.** |
| `MODEL_SPEED` | `1.0` | Speaking rate, passed to Supertonic's duration predictor. Values away from `1.0` (especially >1.2) cause audible distortion on unusual text (numbers, abbreviations, symbols). Leave at `1.0` unless you know you want it. |
| `SUPERTONIC_ONNX_DIR` | `/app/assets/onnx` | Directory containing the ONNX model files |
| `SUPERTONIC_VOICES_DIR` | `/app/assets/voice_styles` | Directory containing `*.json` voice style files |

## Building from source

Requires a Rust toolchain (1.85+ for edition 2024).

```bash
git clone https://github.com/domdomegg/wyoming-supertonic.git
cd wyoming-supertonic
./scripts/download-assets.sh    # fetches ~260 MB from Hugging Face
cargo build --release
./target/release/wyoming-supertonic
```

## Performance

Synthesis time on an Intel i5-8250U (2018-era ULV laptop CPU), CPU-only, voice F2 at `TOTAL_STEPS=3`. Piper (`en_GB-jenny_dioco-medium`) on the same machine for comparison:

| Phrase | Supertonic | Piper |
|---|---|---|
| Short notification | 0.10 s | **0.08 s** |
| Two-sentence reply | **0.13 s** | 0.15 s |
| Morning briefing (~6 s of audio) | **0.27 s** | 0.34 s |
| Long passage (~10 s of audio) | **0.35 s** | 0.40 s |

<details>
<summary>Exact phrases used</summary>

- *"The dryer has finished."*
- *"The front door is unlocked. Locking now."*
- *"Good morning. Today looks sunny, with a high of eighteen degrees. Your first meeting is at nine thirty with the design team."*
- *"Reading time. Once upon a time, there was a small fox who lived at the edge of a great forest, and every morning she greeted the sun with a yawn and a stretch."*

</details>

## Licence

This project is licensed under the [Apache License 2.0](LICENSE). `src/helper.rs` is adapted from the [Supertonic reference Rust implementation](https://github.com/supertone-inc/supertonic/tree/main/rust), also Apache-2.0. See `NOTICE` for attribution.

The Supertonic-2 ONNX weights bundled in the runtime image are distributed under their own terms — see the [model card on Hugging Face](https://huggingface.co/Supertone/supertonic-2).
