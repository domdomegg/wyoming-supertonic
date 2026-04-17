//! Pitch-preserving time-stretch for i16 PCM audio, via the Signalsmith
//! Stretch library.

use signalsmith_stretch::Stretch;

/// Change playback speed without changing pitch.
///
/// `factor > 1.0` = faster / shorter output; `factor < 1.0` = slower / longer.
/// `factor == 1.0` returns the input unchanged.
pub fn time_stretch(samples: &[i16], factor: f32, sample_rate: u32) -> Vec<i16> {
    if (factor - 1.0).abs() < 1e-3 || samples.is_empty() {
        return samples.to_vec();
    }

    let output_len = ((samples.len() as f32) / factor).round() as usize;

    let input_f32: Vec<f32> = samples.iter().map(|&s| s as f32 / 32768.0).collect();
    let mut output_f32 = vec![0f32; output_len];

    let mut stretch = Stretch::preset_default(1, sample_rate);
    stretch.exact(input_f32, &mut output_f32);

    output_f32
        .into_iter()
        .map(|s| (s * 32768.0).clamp(-32768.0, 32767.0) as i16)
        .collect()
}
