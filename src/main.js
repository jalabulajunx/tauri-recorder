const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { save } = window.__TAURI__.dialog;

// DOM Elements
const startBtn = document.getElementById('startBtn');
const pauseBtn = document.getElementById('pauseBtn');
const stopBtn = document.getElementById('stopBtn');
const saveBtn = document.getElementById('saveBtn');
const clearBtn = document.getElementById('clearBtn');
const statusIndicator = document.getElementById('statusIndicator');
const statusDot = statusIndicator.querySelector('.status-dot');
const statusText = document.getElementById('statusText');
const timeDisplay = document.getElementById('timeDisplay');
const sampleRateEl = document.getElementById('sampleRate');
const channelsEl = document.getElementById('channels');
const bufferSizeEl = document.getElementById('bufferSize');
const messageBox = document.getElementById('messageBox');

// State
let isRecording = false;
let isPaused = false;
let timerInterval = null;
let elapsedSeconds = 0;

// Show message
function showMessage(message, type = 'info') {
  messageBox.textContent = message;
  messageBox.className = `message-box ${type}`;
  setTimeout(() => {
    messageBox.textContent = '';
    messageBox.className = 'message-box';
  }, 5000);
}

// Format time
function formatTime(seconds) {
  const hrs = Math.floor(seconds / 3600);
  const mins = Math.floor((seconds % 3600) / 60);
  const secs = seconds % 60;
  return `${hrs.toString().padStart(2, '0')}:${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`;
}

// Update timer display
function updateTimer() {
  elapsedSeconds++;
  timeDisplay.textContent = formatTime(elapsedSeconds);
}

// Update UI based on recording state
function updateUI(state) {
  const { is_recording, is_paused, sample_rate, channels, buffer_size, duration_seconds } = state;

  isRecording = is_recording;
  isPaused = is_paused;

  // Update status indicator
  statusDot.className = 'status-dot';
  if (isRecording) {
    if (isPaused) {
      statusDot.classList.add('paused');
      statusText.textContent = 'Paused';
    } else {
      statusDot.classList.add('recording');
      statusText.textContent = 'Recording';
    }
  } else {
    statusText.textContent = 'Ready';
  }

  // Update buttons
  startBtn.disabled = isRecording;
  pauseBtn.disabled = !isRecording;
  stopBtn.disabled = !isRecording;
  saveBtn.disabled = isRecording || buffer_size === 0;
  clearBtn.disabled = isRecording || buffer_size === 0;

  // Update pause button text
  pauseBtn.innerHTML = isPaused 
    ? '<span class="icon">▶</span> Resume' 
    : '<span class="icon">⏸</span> Pause';

  // Update info
  sampleRateEl.textContent = sample_rate ? `${sample_rate} Hz` : '--';
  channelsEl.textContent = channels || '--';
  bufferSizeEl.textContent = buffer_size ? buffer_size.toLocaleString() : '--';

  // Update timer
  if (duration_seconds !== undefined && duration_seconds > 0) {
    elapsedSeconds = duration_seconds;
    timeDisplay.textContent = formatTime(elapsedSeconds);
  }
}

// Start recording
async function startRecording() {
  try {
    const result = await invoke('start_recording');
    showMessage(result, 'success');
    elapsedSeconds = 0;
    timerInterval = setInterval(updateTimer, 1000);
    await refreshStatus();
  } catch (error) {
    showMessage(`Error: ${error}`, 'error');
  }
}

// Stop recording
async function stopRecording() {
  try {
    const result = await invoke('stop_recording');
    clearInterval(timerInterval);
    showMessage(result, 'success');
    await refreshStatus();
  } catch (error) {
    showMessage(`Error: ${error}`, 'error');
  }
}

// Pause/Resume recording
async function togglePause() {
  try {
    const result = await invoke(isPaused ? 'resume_recording' : 'pause_recording');
    showMessage(result, 'info');
    await refreshStatus();
  } catch (error) {
    showMessage(`Error: ${error}`, 'error');
  }
}

// Save recording
async function saveRecording() {
  try {
    const filePath = await save({
      defaultPath: `recording_${new Date().toISOString().replace(/[:.]/g, '-')}.ogg`,
      filters: [
        { name: 'OGG Audio', extensions: ['ogg'] },
        { name: 'WAV Audio', extensions: ['wav'] }
      ]
    });

    if (filePath) {
      const result = await invoke('save_recording', { path: filePath });
      showMessage(result, 'success');
    }
  } catch (error) {
    showMessage(`Error: ${error}`, 'error');
  }
}

// Clear recording
async function clearRecording() {
  try {
    const result = await invoke('clear_recording');
    elapsedSeconds = 0;
    timeDisplay.textContent = '00:00:00';
    showMessage(result, 'info');
    await refreshStatus();
  } catch (error) {
    showMessage(`Error: ${error}`, 'error');
  }
}

// Refresh status
async function refreshStatus() {
  try {
    const status = await invoke('get_recording_status');
    updateUI(status);
  } catch (error) {
    console.error('Failed to get status:', error);
  }
}

// Listen for recording stopped event
listen('recording-stopped', async () => {
  clearInterval(timerInterval);
  await refreshStatus();
  showMessage('Recording stopped', 'info');
});

// Event listeners
startBtn.addEventListener('click', startRecording);
pauseBtn.addEventListener('click', togglePause);
stopBtn.addEventListener('click', stopRecording);
saveBtn.addEventListener('click', saveRecording);
clearBtn.addEventListener('click', clearRecording);

// Initial status refresh
refreshStatus();

// Periodic status update
setInterval(refreshStatus, 1000);
