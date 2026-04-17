//! Wyoming TTS server wrapping Supertonic-2.
//!
//! Advertises every voice found in the voices directory at startup, but
//! lazy-loads each voice's weights on first use. Typical Home Assistant
//! usage (one voice) keeps resident memory around ~290 MB.
//!
//! Synthesis is serialised via a Mutex on the TTS engine (Supertonic is
//! sub-second, so concurrency doesn't buy us anything on a single CPU).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, RwLock};

mod helper;
mod stretch;
use helper::{Style, TextToSpeech, load_text_to_speech, load_voice_style};

const CHUNK_SAMPLES: usize = 4096;

/// Turn a Supertonic voice ID (e.g. "F2") into a human label (e.g. "Female 2").
fn voice_description(id: &str) -> String {
    let mut chars = id.chars();
    match chars.next() {
        Some('F') => format!("Female {}", chars.as_str()),
        Some('M') => format!("Male {}", chars.as_str()),
        _ => id.to_string(),
    }
}

/// Scan a directory for available voice IDs (filename stems of `*.json` files).
fn discover_voices(dir: &str) -> Result<BTreeMap<String, PathBuf>> {
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading voices dir {}", dir))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if let Some(id) = path.file_stem().and_then(|s| s.to_str())
            && !id.is_empty()
        {
            out.insert(id.to_string(), path);
        }
    }
    Ok(out)
}

#[derive(Debug, Deserialize, Serialize)]
struct Header {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    data_length: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    payload_length: Option<usize>,
}

async fn read_event<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
) -> Result<Option<Header>> {
    let mut line = String::new();
    let n = reader.read_line(&mut line).await?;
    if n == 0 {
        return Ok(None);
    }
    let mut hdr: Header = serde_json::from_str(line.trim())?;
    if let Some(dl) = hdr.data_length
        && dl > 0
    {
        let mut buf = vec![0u8; dl];
        reader.read_exact(&mut buf).await?;
        hdr.data = Some(serde_json::from_slice(&buf)?);
    }
    if let Some(pl) = hdr.payload_length
        && pl > 0
    {
        let mut buf = vec![0u8; pl];
        reader.read_exact(&mut buf).await?;
    }
    Ok(Some(hdr))
}

async fn write_event<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    event_type: &str,
    data: Option<Value>,
    payload: Option<&[u8]>,
) -> Result<()> {
    let data_bytes = data.as_ref().map(serde_json::to_vec).transpose()?;
    let mut hdr = json!({ "type": event_type });
    if let Some(d) = &data_bytes {
        hdr["data_length"] = json!(d.len());
    }
    if let Some(p) = payload {
        hdr["payload_length"] = json!(p.len());
    }
    let mut line = serde_json::to_vec(&hdr)?;
    line.push(b'\n');
    writer.write_all(&line).await?;
    if let Some(d) = data_bytes {
        writer.write_all(&d).await?;
    }
    if let Some(p) = payload {
        writer.write_all(p).await?;
    }
    writer.flush().await?;
    Ok(())
}

struct Config {
    sample_rate: i32,
    total_steps: usize,
    model_speed: f32,
    transform_speed: f32,
    default_voice: String,
}

/// Voice registry: paths are known at startup, but styles load lazily and
/// stay cached for the lifetime of the process.
struct VoiceRegistry {
    paths: BTreeMap<String, PathBuf>,
    cache: RwLock<BTreeMap<String, Arc<Style>>>,
}

impl VoiceRegistry {
    fn new(paths: BTreeMap<String, PathBuf>) -> Self {
        Self {
            paths,
            cache: RwLock::new(BTreeMap::new()),
        }
    }

    fn has(&self, name: &str) -> bool {
        self.paths.contains_key(name)
    }

    fn names(&self) -> impl Iterator<Item = &str> {
        self.paths.keys().map(|s| s.as_str())
    }

    /// Load and cache the given voice. Returns the cached style on hit.
    async fn get_or_load(&self, name: &str) -> Result<Arc<Style>> {
        if let Some(style) = self.cache.read().await.get(name) {
            return Ok(Arc::clone(style));
        }
        let mut cache = self.cache.write().await;
        if let Some(style) = cache.get(name) {
            return Ok(Arc::clone(style));
        }
        let path = self
            .paths
            .get(name)
            .with_context(|| format!("unknown voice {}", name))?;
        let path_str = path.to_string_lossy().into_owned();
        let style = load_voice_style(std::slice::from_ref(&path_str), false)
            .with_context(|| format!("loading voice {}", name))?;
        let arc = Arc::new(style);
        cache.insert(name.to_string(), Arc::clone(&arc));
        eprintln!("loaded voice {} from {}", name, path_str);
        Ok(arc)
    }
}

async fn handle_synthesize(
    write_half: &mut tokio::net::tcp::OwnedWriteHalf,
    text: &str,
    voice_name: &str,
    tts: &Arc<Mutex<TextToSpeech>>,
    voices: &Arc<VoiceRegistry>,
    cfg: &Config,
) -> Result<()> {
    let resolved = if voices.has(voice_name) {
        voice_name
    } else {
        cfg.default_voice.as_str()
    };
    let style = voices.get_or_load(resolved).await?;

    write_event(
        write_half,
        "audio-start",
        Some(json!({ "rate": cfg.sample_rate, "width": 2, "channels": 1 })),
        None,
    )
    .await?;

    let samples = {
        let mut guard = tts.lock().await;
        let (wav, _dur) = guard.call(text, "en", &style, cfg.total_steps, cfg.model_speed, 0.3)?;
        wav
    };

    let mut i16_samples: Vec<i16> = samples
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect();
    if (cfg.transform_speed - 1.0).abs() > 1e-3 {
        i16_samples =
            stretch::time_stretch(&i16_samples, cfg.transform_speed, cfg.sample_rate as u32);
    }

    let mut pcm = Vec::with_capacity(i16_samples.len() * 2);
    for s in &i16_samples {
        pcm.extend_from_slice(&s.to_le_bytes());
    }

    let chunk_bytes = CHUNK_SAMPLES * 2;
    for chunk in pcm.chunks(chunk_bytes) {
        write_event(
            write_half,
            "audio-chunk",
            Some(json!({ "rate": cfg.sample_rate, "width": 2, "channels": 1 })),
            Some(chunk),
        )
        .await?;
    }
    write_event(write_half, "audio-stop", None, None).await?;
    Ok(())
}

fn build_info(voices: &VoiceRegistry) -> Value {
    let voice_list: Vec<Value> = voices
        .names()
        .map(|id| {
            json!({
                "name": id,
                "description": voice_description(id),
                "attribution": {"name": "Supertone", "url": "https://huggingface.co/Supertone/supertonic-2"},
                "installed": true,
                "languages": ["en"],
                "version": "v2"
            })
        })
        .collect();

    json!({
        "tts": [{
            "name": "supertonic",
            "description": "Supertonic-2 TTS",
            "attribution": {"name": "Supertone", "url": "https://github.com/supertone-inc/supertonic"},
            "installed": true,
            "version": env!("CARGO_PKG_VERSION"),
            "voices": voice_list
        }]
    })
}

async fn handle_client(
    stream: TcpStream,
    tts: Arc<Mutex<TextToSpeech>>,
    voices: Arc<VoiceRegistry>,
    info: Arc<Value>,
    cfg: Arc<Config>,
) -> Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    while let Some(hdr) = read_event(&mut reader).await? {
        match hdr.event_type.as_str() {
            "describe" => {
                write_event(&mut write_half, "info", Some((*info).clone()), None).await?;
            }
            "synthesize" => {
                let data = hdr.data.as_ref();
                let text = data
                    .and_then(|d| d.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                if text.is_empty() {
                    continue;
                }
                let voice_name = data
                    .and_then(|d| d.get("voice"))
                    .and_then(|v| v.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or(&cfg.default_voice);
                handle_synthesize(&mut write_half, text, voice_name, &tts, &voices, &cfg).await?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn env_or<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

#[tokio::main]
async fn main() -> Result<()> {
    let onnx_dir =
        std::env::var("SUPERTONIC_ONNX_DIR").unwrap_or_else(|_| "assets/onnx".to_string());
    let voices_dir = std::env::var("SUPERTONIC_VOICES_DIR")
        .unwrap_or_else(|_| "assets/voice_styles".to_string());
    let default_voice =
        std::env::var("SUPERTONIC_DEFAULT_VOICE").unwrap_or_else(|_| "F2".to_string());
    let port: u16 = env_or("PORT", 10220u16);
    let total_steps: usize = env_or("TOTAL_STEPS", 3usize);
    // MODEL_SPEED tweaks the duration predictor inside Supertonic. Values far
    // from 1.0 hurt quality — see the README. Prefer TRANSFORM_SPEED.
    let model_speed: f32 = env_or("MODEL_SPEED", 1.0f32);
    // TRANSFORM_SPEED time-stretches the rendered audio. Preserves pitch.
    let transform_speed: f32 = env_or("TRANSFORM_SPEED", 1.4f32);

    eprintln!("Loading Supertonic engine from {}", onnx_dir);
    let tts = load_text_to_speech(&onnx_dir, false)?;
    let sample_rate = tts.sample_rate;

    eprintln!("Discovering voices in {}", voices_dir);
    let paths = discover_voices(&voices_dir)?;
    if paths.is_empty() {
        anyhow::bail!("no voice styles found in {}", voices_dir);
    }
    if !paths.contains_key(&default_voice) {
        anyhow::bail!(
            "configured default voice {} not found; available: {:?}",
            default_voice,
            paths.keys().collect::<Vec<_>>()
        );
    }
    eprintln!(
        "Advertising {} voices ({:?}); default={} (loaded lazily on first use) sample_rate={} total_steps={} model_speed={} transform_speed={}",
        paths.len(),
        paths.keys().collect::<Vec<_>>(),
        default_voice,
        sample_rate,
        total_steps,
        model_speed,
        transform_speed,
    );

    let voices = Arc::new(VoiceRegistry::new(paths));
    let info = Arc::new(build_info(&voices));
    let cfg = Arc::new(Config {
        sample_rate,
        total_steps,
        model_speed,
        transform_speed,
        default_voice,
    });
    let tts = Arc::new(Mutex::new(tts));

    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await?;
    eprintln!("Listening on {}", addr);

    loop {
        let (stream, peer) = listener.accept().await?;
        eprintln!("connection from {}", peer);
        let tts = Arc::clone(&tts);
        let voices = Arc::clone(&voices);
        let info = Arc::clone(&info);
        let cfg = Arc::clone(&cfg);
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, tts, voices, info, cfg).await {
                eprintln!("client error: {:#}", e);
            }
        });
    }
}
