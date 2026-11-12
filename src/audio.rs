use std::{
    fs::File,
    io::{self, BufWriter},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
        mpsc,
    },
    thread,
};

use anyhow::{Context, Result, bail, ensure};
use cpal::{
    Device, FromSample, SampleFormat, SizedSample, Stream, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

type WavWriter = hound::WavWriter<BufWriter<File>>;

pub fn list_inputs() -> Result<()> {
    let host = cpal::default_host();
    if let Some(device) = host.default_input_device() {
        println!("Default input: {}", device.name()?);
    }
    for (index, device) in host.input_devices()?.enumerate() {
        println!("{index}: {}", device.name()?);
    }
    Ok(())
}

pub fn input_device(index: Option<usize>) -> Result<Device> {
    let host = cpal::default_host();
    match index {
        Some(index) => host
            .input_devices()?
            .nth(index)
            .with_context(|| format!("Input {index} not found. Run `lovelace microphones` to list inputs.")),
        None => host.default_input_device().context(
            "No default microphone found. Check your system audio input and microphone permissions.",
        ),
    }
}

pub fn record(device: &Device, path: &Path) -> Result<()> {
    record_until(device, path, None, |stop| {
        let name = device.name().unwrap_or_else(|_| "audio input".into());
        println!("Recording from {name}. Press Enter to stop.");
        thread::spawn(move || {
            let result = io::stdin()
                .read_line(&mut String::new())
                .map(|_| ())
                .context("Could not read terminal input");
            let _ = stop.try_send(result);
        });
    })
}

pub fn record_until(
    device: &Device,
    path: &Path,
    peak: Option<Arc<AtomicU32>>,
    ready: impl FnOnce(mpsc::SyncSender<Result<()>>),
) -> Result<()> {
    let input = device
        .default_input_config()
        .context("Could not get the microphone format")?;
    let config = input.config();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: config.sample_rate.0,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let writer = Arc::new(Mutex::new(hound::WavWriter::create(path, spec)?));
    let (stop, stopped) = mpsc::sync_channel(1);

    let stream = match input.sample_format() {
        SampleFormat::F32 => input_stream::<f32>(device, &config, &writer, &stop, peak),
        SampleFormat::F64 => input_stream::<f64>(device, &config, &writer, &stop, peak),
        SampleFormat::I8 => input_stream::<i8>(device, &config, &writer, &stop, peak),
        SampleFormat::I16 => input_stream::<i16>(device, &config, &writer, &stop, peak),
        SampleFormat::I32 => input_stream::<i32>(device, &config, &writer, &stop, peak),
        SampleFormat::I64 => input_stream::<i64>(device, &config, &writer, &stop, peak),
        SampleFormat::U8 => input_stream::<u8>(device, &config, &writer, &stop, peak),
        SampleFormat::U16 => input_stream::<u16>(device, &config, &writer, &stop, peak),
        SampleFormat::U32 => input_stream::<u32>(device, &config, &writer, &stop, peak),
        SampleFormat::U64 => input_stream::<u64>(device, &config, &writer, &stop, peak),
        format => bail!("Unsupported microphone format ({format})"),
    }
    .context("Could not open the microphone")?;
    stream.play().context("Could not start audio capture")?;
    ready(stop);
    let result = stopped.recv();
    drop(stream);
    let writer = Arc::into_inner(writer)
        .context("The microphone is still using the audio file")?
        .into_inner()
        .map_err(|_| anyhow::anyhow!("Recording interrupted"))?;
    let samples = writer.duration();
    writer.finalize()?;
    result?.with_context(|| format!("Recording interrupted. WAV saved to {}", path.display()))?;
    ensure!(
        samples > 0,
        "The microphone did not provide any audio samples"
    );
    Ok(())
}

fn input_stream<T>(
    device: &Device,
    config: &StreamConfig,
    writer: &Arc<Mutex<WavWriter>>,
    stop: &mpsc::SyncSender<Result<()>>,
    peak: Option<Arc<AtomicU32>>,
) -> Result<Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels);
    ensure!(channels > 0, "The microphone reported zero channels");
    let writer = Arc::clone(writer);
    let write_error = stop.clone();
    let stream_error = stop.clone();
    Ok(device.build_input_stream(
        config,
        move |data: &[T], _| {
            let Ok(mut writer) = writer.lock() else {
                let _ = write_error.try_send(Err(anyhow::anyhow!("Recording failed")));
                return;
            };
            let mut level = 0.0_f32;
            for frame in data.chunks_exact(channels) {
                let mono = frame
                    .iter()
                    .map(|sample| sample.to_sample::<f32>())
                    .sum::<f32>()
                    / channels as f32;
                let pcm = (mono.clamp(-1.0, 1.0) * 32768.0) as i16;
                if peak.is_some() {
                    level = level.max(mono.abs());
                }
                if let Err(error) = writer.write_sample(pcm) {
                    let _ =
                        write_error.try_send(Err(error).context("Could not write the WAV file"));
                    break;
                }
            }
            if let Some(peak) = &peak {
                peak.fetch_max(level.min(1.0).to_bits(), Ordering::Relaxed);
            }
        },
        move |error| {
            let _ =
                stream_error.try_send(Err(error).context("The microphone stopped capturing audio"));
        },
        None,
    )?)
}
