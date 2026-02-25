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
        pub fn new() -> Result<Self, String> {
            unsafe {
                // Initialize COM
                CoInitializeEx(None, COINIT_MULTITHREADED).ok()
                    .map_err(|e| format!("COM initialization failed: {}", e))?;

                // Get default audio endpoint (render device for loopback)
                let enumerator: IMMDeviceEnumerator =
                    CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                        .map_err(|e| format!("Failed to create device enumerator: {}", e))?;

                let device = enumerator
                    .GetDefaultAudioEndpoint(eRender, eConsole)
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

                // Initialize audio client for loopback capture
                let stream_flags = AUDCLNT_STREAMFLAGS_LOOPBACK;
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
use windows_audio::ThreadSafeCapture;

/// Start recording system audio (loopback capture)
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

        // Initialize WASAPI loopback capture
        let capture = ThreadSafeCapture::new()?;
        let (sample_rate, channels) = capture.get_format();
        *SAMPLE_RATE.lock() = sample_rate;
        *CHANNELS.lock() = channels;

        capture.start()?;
        RECORDING.store(true, Ordering::SeqCst);
        PAUSED.store(false, Ordering::SeqCst);

        // Spawn recording thread
        let app_handle = app.clone();
        std::thread::spawn(move || {
            loop {
                if !RECORDING.load(Ordering::SeqCst) {
                    break;
                }

                if !PAUSED.load(Ordering::SeqCst) {
                    match capture.read_buffer() {
                        Ok(samples) => {
                            if !samples.is_empty() {
                                AUDIO_BUFFER.lock().extend(samples);
                            }
                        }
                        Err(e) => {
                            eprintln!("Error reading buffer: {}", e);
                        }
                    }
                }

                std::thread::sleep(Duration::from_millis(10));
            }

            let _ = capture.stop();

            // Emit recording stopped event
            let _ = app_handle.emit("recording-stopped", ());
        });

        Ok(format!(
            "Recording started at {} Hz, {} channels",
            sample_rate, channels
        ))
    }

    #[cfg(not(windows))]
    {
        let _ = app;
        Err("This application only works on Windows".to_string())
    }
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
    use libopusenc::{OpusEncoder, OpusEncComments, OpusEncSampleRate, OpusEncChannelMapping};
    
    // Create comments (empty for now)
    let mut comments = OpusEncComments::new()
        .map_err(|e| format!("Failed to create comments: {:?}", e))?;
    
    // Map sample rate to enum
    let rate = match sample_rate {
        48000 => OpusEncSampleRate::Rate48000,
        24000 => OpusEncSampleRate::Rate24000,
        16000 => OpusEncSampleRate::Rate16000,
        12000 => OpusEncSampleRate::Rate12000,
        8000 => OpusEncSampleRate::Rate8000,
        _ => OpusEncSampleRate::Rate48000, // Default to 48kHz
    };
    
    // Create encoder
    let mut encoder = OpusEncoder::create_file(
        path,
        &mut comments,
        rate,
        channels as u8,
        OpusEncChannelMapping::Voice,
    ).map_err(|e| format!("Failed to create encoder: {:?}", e))?;
    
    // Convert f32 to i16
    let mut pcm_data: Vec<i16> = Vec::with_capacity(buffer.len());
    for &sample in buffer {
        let s = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
        pcm_data.push(s);
    }
    
    // Encode the entire buffer
    encoder.encode(&pcm_data)
        .map_err(|e| format!("Failed to encode: {:?}", e))?;
    
    // Finish
    encoder.finalize()
        .map_err(|e| format!("Failed to finalize: {:?}", e))?;
    
    Ok(())
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
