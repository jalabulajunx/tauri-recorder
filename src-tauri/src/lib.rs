use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::Emitter;

// Global recording state
static RECORDING: AtomicBool = AtomicBool::new(false);
static PAUSED: AtomicBool = AtomicBool::new(false);

// Audio buffer for storing recorded samples
lazy_static::lazy_static! {
    static ref AUDIO_BUFFER: Mutex<Vec<f32>> = Mutex::new(Vec::new());
    static ref SAMPLE_RATE: Mutex<u32> = Mutex::new(48000);
    static ref CHANNELS: Mutex<u16> = Mutex::new(2);
    static ref RECORDING_START: Mutex<Option<Instant>> = Mutex::new(None);
}

#[cfg(windows)]
mod windows_audio {
    use std::ptr::null_mut;
    use std::sync::atomic::{AtomicPtr, Ordering};
    use windows::core::Interface;
    use windows::Win32::Media::Audio::*;
    use windows::Win32::System::Com::*;

    /// Which audio stream to capture.
    pub enum CaptureMode {
        /// System audio output (what plays through speakers/headphones).
        Loopback,
        /// Default microphone input.
        Microphone,
    }

    // Thread-safe capture using raw pointers
    pub struct ThreadSafeCapture {
        audio_client: AtomicPtr<std::ffi::c_void>,
        capture_client: AtomicPtr<std::ffi::c_void>,
        sample_rate: u32,
        channels: u16,
    }

    // Safety: We manage the raw pointers manually and only access them from one thread at a time
    unsafe impl Send for ThreadSafeCapture {}
    unsafe impl Sync for ThreadSafeCapture {}

    impl ThreadSafeCapture {
        pub fn new(mode: CaptureMode) -> Result<Self, String> {
            unsafe {
                // Initialize COM (S_FALSE if already initialised — that's fine)
                CoInitializeEx(None, COINIT_MULTITHREADED).ok()
                    .map_err(|e| format!("COM initialization failed: {}", e))?;

                let enumerator: IMMDeviceEnumerator =
                    CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                        .map_err(|e| format!("Failed to create device enumerator: {}", e))?;

                // Pick endpoint + stream flags based on mode
                let (data_flow, stream_flags) = match mode {
                    CaptureMode::Loopback   => (eRender,  AUDCLNT_STREAMFLAGS_LOOPBACK),
                    CaptureMode::Microphone => (eCapture, 0u32),
                };

                let device = enumerator
                    .GetDefaultAudioEndpoint(data_flow, eConsole)
                    .map_err(|e| format!("Failed to get default audio endpoint: {}", e))?;

                // Activate audio client
                let audio_client: IAudioClient = device
                    .Activate(CLSCTX_ALL, None)
                    .map_err(|e| format!("Failed to activate audio client: {}", e))?;

                // Get mix format
                let format_ptr = audio_client
                    .GetMixFormat()
                    .map_err(|e| format!("Failed to get mix format: {}", e))?;

                let format = &*format_ptr;
                let sample_rate = format.nSamplesPerSec;
                let channels = format.nChannels;

                let duration = 10_000_000;

                audio_client
                    .Initialize(
                        AUDCLNT_SHAREMODE_SHARED,
                        stream_flags,
                        duration,
                        0,
                        format_ptr,
                        None,
                    )
                    .map_err(|e| format!("Failed to initialize audio client: {}", e))?;

                // Get capture client
                let capture_client = audio_client
                    .GetService::<IAudioCaptureClient>()
                    .map_err(|e| format!("Failed to get capture client: {}", e))?;

                // Store raw pointers
                let audio_client_ptr = audio_client.as_raw() as *mut std::ffi::c_void;
                let capture_client_ptr = capture_client.as_raw() as *mut std::ffi::c_void;

                // Don't drop the COM objects - we'll manage them manually
                std::mem::forget(audio_client);
                std::mem::forget(capture_client);

                Ok(Self {
                    audio_client: AtomicPtr::new(audio_client_ptr),
                    capture_client: AtomicPtr::new(capture_client_ptr),
                    sample_rate,
                    channels,
                })
            }
        }

        pub fn get_format(&self) -> (u32, u16) {
            (self.sample_rate, self.channels)
        }

        pub fn start(&self) -> Result<(), String> {
            unsafe {
                let ptr = self.audio_client.load(Ordering::SeqCst);
                let audio_client: IAudioClient = Interface::from_raw(ptr as *mut _);
                audio_client
                    .Start()
                    .map_err(|e| format!("Failed to start audio client: {}", e))?;
                // Don't decrement ref count
                std::mem::forget(audio_client);
                Ok(())
            }
        }

        pub fn stop(&self) -> Result<(), String> {
            unsafe {
                let ptr = self.audio_client.load(Ordering::SeqCst);
                let audio_client: IAudioClient = Interface::from_raw(ptr as *mut _);
                let result = audio_client
                    .Stop()
                    .map_err(|e| format!("Failed to stop audio client: {}", e));
                std::mem::forget(audio_client);
                result
            }
        }

        pub fn read_buffer(&self) -> Result<Vec<f32>, String> {
            unsafe {
                let ptr = self.capture_client.load(Ordering::SeqCst);
                let capture_client: IAudioCaptureClient = Interface::from_raw(ptr as *mut _);
                
                let mut buffer = Vec::new();
                let mut frames_available = true;

                while frames_available {
                    let packet_length = capture_client.GetNextPacketSize()
                        .map_err(|e| format!("Failed to get packet size: {}", e))?;

                    if packet_length == 0 {
                        frames_available = false;
                        continue;
                    }

                    let mut data_ptr: *mut u8 = null_mut();
                    let mut num_frames = 0u32;
                    let mut flags = 0u32;

                    capture_client
                        .GetBuffer(&mut data_ptr, &mut num_frames, &mut flags, None, None)
                        .map_err(|e| format!("Failed to get buffer: {}", e))?;

                    if data_ptr.is_null() || num_frames == 0 {
                        frames_available = false;
                        continue;
                    }

                    let samples = data_ptr as *const f32;
                    let total_samples = (num_frames * self.channels as u32) as usize;

                    for i in 0..total_samples {
                        let sample = *samples.add(i);
                        if flags & 0x1 != 0 {
                            buffer.push(0.0);
                        } else {
                            buffer.push(sample);
                        }
                    }

                    capture_client
                        .ReleaseBuffer(num_frames)
                        .map_err(|e| format!("Failed to release buffer: {}", e))?;
                }

                std::mem::forget(capture_client);
                Ok(buffer)
            }
        }
    }

    impl Drop for ThreadSafeCapture {
        fn drop(&mut self) {
            unsafe {
                // Properly release COM objects
                let audio_ptr = self.audio_client.load(Ordering::SeqCst);
                let capture_ptr = self.capture_client.load(Ordering::SeqCst);
                
                if !audio_ptr.is_null() {
                    let _audio: IAudioClient = Interface::from_raw(audio_ptr as *mut _);
                }
                if !capture_ptr.is_null() {
                    let _capture: IAudioCaptureClient = Interface::from_raw(capture_ptr as *mut _);
                }
                CoUninitialize();
            }
        }
    }
}

#[cfg(windows)]
use windows_audio::{ThreadSafeCapture, CaptureMode};

/// Start recording system audio (loopback + microphone)
#[tauri::command]
async fn start_recording(app: tauri::AppHandle) -> Result<String, String> {
    if RECORDING.load(Ordering::SeqCst) {
        return Err("Already recording".to_string());
    }

    #[cfg(windows)]
    {
        // Clear previous buffer
        AUDIO_BUFFER.lock().clear();
        *RECORDING_START.lock() = Some(Instant::now());

        // Initialize WASAPI loopback capture (speakers → remote person's voice)
        let loopback = ThreadSafeCapture::new(CaptureMode::Loopback)?;
        let (sample_rate, channels) = loopback.get_format();
        *SAMPLE_RATE.lock() = sample_rate;
        *CHANNELS.lock() = channels;

        // Initialize microphone capture (local person's voice)
        let mic = match ThreadSafeCapture::new(CaptureMode::Microphone) {
            Ok(m) => {
                eprintln!("Microphone capture initialised: {} Hz, {} ch",
                          m.get_format().0, m.get_format().1);
                Some(m)
            }
            Err(e) => {
                eprintln!("WARNING: Could not open microphone — only loopback will be recorded: {}", e);
                None
            }
        };

        loopback.start()?;
        if let Some(ref m) = mic { m.start()?; }

        RECORDING.store(true, Ordering::SeqCst);
        PAUSED.store(false, Ordering::SeqCst);

        let loopback_channels = channels;
        let has_mic = mic.is_some();

        // Spawn recording thread
        let app_handle = app.clone();
        std::thread::spawn(move || {
            loop {
                if !RECORDING.load(Ordering::SeqCst) {
                    break;
                }

                if !PAUSED.load(Ordering::SeqCst) {
                    // Read loopback (speaker output — remote voice)
                    let loopback_samples = match loopback.read_buffer() {
                        Ok(s) => s,
                        Err(e) => { eprintln!("Loopback read error: {}", e); vec![] }
                    };

                    // Read microphone (local voice)
                    let mic_samples = if let Some(ref m) = mic {
                        match m.read_buffer() {
                            Ok(s) => s,
                            Err(e) => { eprintln!("Mic read error: {}", e); vec![] }
                        }
                    } else {
                        vec![]
                    };

                    // Mix: sum the two streams sample-by-sample.
                    // If mic has a different channel count, up/down-mix naively.
                    let mic_ch = mic.as_ref().map(|m| m.get_format().1).unwrap_or(loopback_channels);

                    if !loopback_samples.is_empty() || !mic_samples.is_empty() {
                        let mixed = mix_streams(
                            &loopback_samples, loopback_channels,
                            &mic_samples, mic_ch,
                            loopback_channels,
                        );
                        AUDIO_BUFFER.lock().extend(mixed);
                    }
                }

                std::thread::sleep(Duration::from_millis(10));
            }

            let _ = loopback.stop();
            if let Some(ref m) = mic { let _ = m.stop(); }

            // Emit recording stopped event
            let _ = app_handle.emit("recording-stopped", ());
        });

        let mic_status = if has_mic { "+ mic" } else { "(no mic)" };
        Ok(format!(
            "Recording started at {} Hz, {} ch, loopback {}",
            sample_rate, channels, mic_status
        ))
    }

    #[cfg(not(windows))]
    {
        let _ = app;
        Err("This application only works on Windows".to_string())
    }
}

/// Mix two interleaved f32 PCM streams into one.
/// If channel counts differ, mono→stereo is duplicated, stereo→mono is averaged.
fn mix_streams(
    a: &[f32], a_ch: u16,
    b: &[f32], b_ch: u16,
    out_ch: u16,
) -> Vec<f32> {
    let a_frames = if a_ch > 0 { a.len() / a_ch as usize } else { 0 };
    let b_frames = if b_ch > 0 { b.len() / b_ch as usize } else { 0 };
    let out_frames = a_frames.max(b_frames);
    let oc = out_ch as usize;
    let mut out = Vec::with_capacity(out_frames * oc);

    for f in 0..out_frames {
        for c in 0..oc {
            // Get sample from stream A
            let sa = if f < a_frames {
                if a_ch as usize == oc {
                    a[f * a_ch as usize + c]
                } else if a_ch == 1 {
                    a[f] // mono → replicate to every output channel
                } else {
                    // stereo → mono: average
                    let sum: f32 = (0..a_ch as usize).map(|k| a[f * a_ch as usize + k]).sum();
                    sum / a_ch as f32
                }
            } else {
                0.0
            };

            // Get sample from stream B
            let sb = if f < b_frames {
                if b_ch as usize == oc {
                    b[f * b_ch as usize + c]
                } else if b_ch == 1 {
                    b[f]
                } else {
                    let sum: f32 = (0..b_ch as usize).map(|k| b[f * b_ch as usize + k]).sum();
                    sum / b_ch as f32
                }
            } else {
                0.0
            };

            // Sum and soft-clamp to [-1, 1]
            out.push((sa + sb).clamp(-1.0, 1.0));
        }
    }

    out
}

/// Stop recording and save to file
#[tauri::command]
async fn stop_recording() -> Result<String, String> {
    if !RECORDING.load(Ordering::SeqCst) {
        return Err("Not recording".to_string());
    }

    RECORDING.store(false, Ordering::SeqCst);

    // Wait a bit for the recording thread to finish
    std::thread::sleep(Duration::from_millis(100));

    let duration = {
        let guard = RECORDING_START.lock();
        match *guard {
            Some(start) => start.elapsed().as_secs(),
            None => 0,
        }
    };

    Ok(format!("Recording stopped. Duration: {} seconds", duration))
}

/// Pause recording
#[tauri::command]
fn pause_recording() -> Result<String, String> {
    if !RECORDING.load(Ordering::SeqCst) {
        return Err("Not recording".to_string());
    }

    PAUSED.store(true, Ordering::SeqCst);
    Ok("Recording paused".to_string())
}

/// Resume recording
#[tauri::command]
fn resume_recording() -> Result<String, String> {
    if !RECORDING.load(Ordering::SeqCst) {
        return Err("Not recording".to_string());
    }

    PAUSED.store(false, Ordering::SeqCst);
    Ok("Recording resumed".to_string())
}

/// Get recording status
#[tauri::command]
fn get_recording_status() -> serde_json::Value {
    let is_recording = RECORDING.load(Ordering::SeqCst);
    let is_paused = PAUSED.load(Ordering::SeqCst);
    let buffer_size = AUDIO_BUFFER.lock().len();
    let duration = if is_recording {
        let guard = RECORDING_START.lock();
        match *guard {
            Some(start) => start.elapsed().as_secs(),
            None => 0,
        }
    } else {
        0
    };

    serde_json::json!({
        "is_recording": is_recording,
        "is_paused": is_paused,
        "buffer_size": buffer_size,
        "duration_seconds": duration,
        "sample_rate": *SAMPLE_RATE.lock(),
        "channels": *CHANNELS.lock(),
    })
}

/// Save recorded audio to WAV or OGG file
#[tauri::command]
async fn save_recording(path: String) -> Result<String, String> {
    let buffer = AUDIO_BUFFER.lock().clone();
    let sample_rate = *SAMPLE_RATE.lock();
    let channels = *CHANNELS.lock();

    if buffer.is_empty() {
        return Err("No audio data to save".to_string());
    }

    let path_lower = path.to_lowercase();
    
    if path_lower.ends_with(".ogg") || path_lower.ends_with(".opus") {
        // Save as OGG/Opus
        save_as_ogg(&path, &buffer, sample_rate, channels)?;
    } else {
        // Save as WAV (default)
        save_as_wav(&path, &buffer, sample_rate, channels)?;
    }

    let duration_secs = buffer.len() as f32 / (sample_rate as f32 * channels as f32);
    Ok(format!(
        "Saved {} seconds of audio to {}",
        duration_secs, path
    ))
}

fn save_as_wav(path: &str, buffer: &[f32], sample_rate: u32, channels: u16) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };

    let mut writer = hound::WavWriter::create(path, spec)
        .map_err(|e| format!("Failed to create WAV file: {}", e))?;

    for sample in buffer.iter() {
        writer
            .write_sample(*sample)
            .map_err(|e| format!("Failed to write sample: {}", e))?;
    }

    writer
        .finalize()
        .map_err(|e| format!("Failed to finalize WAV file: {}", e))?;

    Ok(())
}

fn save_as_ogg(path: &str, buffer: &[f32], sample_rate: u32, channels: u16) -> Result<(), String> {
    use audiopus::coder::Encoder as OpusEncoder;
    use audiopus::{Application, Channels as OpusChannels, SampleRate as OpusSampleRate, Bitrate};
    use ogg::writing::{PacketWriter, PacketWriteEndInfo};
    use std::fs::File;
    use std::io::BufWriter;

    // Opus supported sample rates: 8000, 12000, 16000, 24000, 48000
    let opus_rate = match sample_rate {
        8000  => OpusSampleRate::Hz8000,
        12000 => OpusSampleRate::Hz12000,
        16000 => OpusSampleRate::Hz16000,
        24000 => OpusSampleRate::Hz24000,
        48000 => OpusSampleRate::Hz48000,
        _ => OpusSampleRate::Hz48000, // will resample below
    };

    let opus_channels = match channels {
        1 => OpusChannels::Mono,
        2 => OpusChannels::Stereo,
        _ => return Err(format!("Unsupported channel count: {}. Opus supports mono/stereo.", channels)),
    };

    // Resample to an Opus-compatible rate if needed (e.g. 44100 → 48000)
    let needs_resample = !matches!(sample_rate, 8000 | 12000 | 16000 | 24000 | 48000);
    let (audio_data, encode_rate) = if needs_resample {
        let resampled = resample_linear(buffer, sample_rate, 48000, channels);
        (resampled, 48000u32)
    } else {
        (buffer.to_vec(), sample_rate)
    };

    // Create Opus encoder
    let mut encoder = OpusEncoder::new(opus_rate, opus_channels, Application::Audio)
        .map_err(|e| format!("Failed to create Opus encoder: {}", e))?;

    // 64 kbps — good quality for meeting recordings, ~30 MB/hr stereo
    encoder.set_bitrate(Bitrate::BitsPerSecond(64_000))
        .map_err(|e| format!("Failed to set bitrate: {}", e))?;

    // Get encoder lookahead (pre-skip) — how many samples the decoder must discard
    let pre_skip = encoder.lookahead()
        .map_err(|e| format!("Failed to get lookahead: {}", e))? as u16;

    // OGG Opus always expresses granule position at 48 kHz
    let pre_skip_48 = if encode_rate == 48000 {
        pre_skip
    } else {
        ((pre_skip as u64) * 48000 / encode_rate as u64) as u16
    };

    // Open output file
    let file = File::create(path)
        .map_err(|e| format!("Failed to create file: {}", e))?;
    let writer = BufWriter::new(file);
    let mut pkt_writer = PacketWriter::new(writer);

    // Stream serial — use nanosecond component of system time
    let serial: u32 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();

    // ── OpusHead (RFC 7845 §5.1) ──────────────────────────────────────
    let mut opus_head = Vec::with_capacity(19);
    opus_head.extend_from_slice(b"OpusHead");                      // magic
    opus_head.push(1);                                             // version
    opus_head.push(channels as u8);                                // channel count
    opus_head.extend_from_slice(&pre_skip_48.to_le_bytes());       // pre-skip (at 48 kHz)
    opus_head.extend_from_slice(&sample_rate.to_le_bytes());       // original sample rate (informational)
    opus_head.extend_from_slice(&0i16.to_le_bytes());              // output gain
    opus_head.push(0);                                             // channel mapping family 0

    pkt_writer.write_packet(opus_head, serial, PacketWriteEndInfo::EndPage, 0)
        .map_err(|e| format!("Failed to write OpusHead: {}", e))?;

    // ── OpusTags (RFC 7845 §5.2) ──────────────────────────────────────
    let vendor = b"tauri-recorder";
    let mut opus_tags = Vec::with_capacity(8 + 4 + vendor.len() + 4);
    opus_tags.extend_from_slice(b"OpusTags");
    opus_tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    opus_tags.extend_from_slice(vendor);
    opus_tags.extend_from_slice(&0u32.to_le_bytes());              // 0 user comments

    pkt_writer.write_packet(opus_tags, serial, PacketWriteEndInfo::EndPage, 0)
        .map_err(|e| format!("Failed to write OpusTags: {}", e))?;

    // ── Audio frames ───────────────────────────────────────────────────
    // 20 ms frames — the sweet spot for quality vs. overhead
    let frame_samples_per_ch = encode_rate as usize / 50;          // 960 @ 48 kHz
    let frame_size = frame_samples_per_ch * channels as usize;     // interleaved total
    let mut output_buf = vec![0u8; 4000];                          // max Opus packet
    let mut granule_pos: u64 = pre_skip_48 as u64;

    // Ratio for converting encode_rate granules to 48 kHz granules
    let granule_samples_per_frame: u64 = if encode_rate == 48000 {
        frame_samples_per_ch as u64
    } else {
        (frame_samples_per_ch as u64) * 48000 / encode_rate as u64
    };

    let total_chunks = if audio_data.is_empty() { 0 } else {
        (audio_data.len() + frame_size - 1) / frame_size
    };

    for (i, chunk) in audio_data.chunks(frame_size).enumerate() {
        let is_last = i + 1 == total_chunks;

        // Pad final frame with silence so Opus gets a complete frame
        let frame: Vec<f32> = if chunk.len() < frame_size {
            let mut padded = chunk.to_vec();
            padded.resize(frame_size, 0.0);
            padded
        } else {
            chunk.to_vec()
        };

        let encoded_len = encoder.encode_float(&frame, &mut output_buf)
            .map_err(|e| format!("Opus encode error on frame {}: {}", i, e))?;

        granule_pos += granule_samples_per_frame;

        let end_info = if is_last {
            PacketWriteEndInfo::EndStream
        } else {
            PacketWriteEndInfo::NormalPacket
        };

        pkt_writer.write_packet(
            output_buf[..encoded_len].to_vec(),
            serial,
            end_info,
            granule_pos,
        ).map_err(|e| format!("Failed to write audio packet {}: {}", i, e))?;
    }

    Ok(())
}

/// Linear-interpolation resampler (good enough for a PoC).
/// Converts interleaved f32 PCM from `from_rate` to `to_rate`.
fn resample_linear(input: &[f32], from_rate: u32, to_rate: u32, channels: u16) -> Vec<f32> {
    let ch = channels as usize;
    let in_frames = input.len() / ch;
    let out_frames = ((in_frames as u64) * (to_rate as u64) / (from_rate as u64)) as usize;
    let mut output = Vec::with_capacity(out_frames * ch);

    for i in 0..out_frames {
        let src_pos = (i as f64) * (from_rate as f64) / (to_rate as f64);
        let idx = src_pos as usize;
        let frac = (src_pos - idx as f64) as f32;

        for c in 0..ch {
            let s0 = input.get(idx * ch + c).copied().unwrap_or(0.0);
            let s1 = input.get((idx + 1) * ch + c).copied().unwrap_or(s0);
            output.push(s0 + (s1 - s0) * frac);
        }
    }

    output
}

/// Clear recorded audio buffer
#[tauri::command]
fn clear_recording() -> Result<String, String> {
    if RECORDING.load(Ordering::SeqCst) {
        return Err("Cannot clear while recording".to_string());
    }

    AUDIO_BUFFER.lock().clear();
    Ok("Recording buffer cleared".to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            start_recording,
            stop_recording,
            pause_recording,
            resume_recording,
            get_recording_status,
            save_recording,
            clear_recording,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
