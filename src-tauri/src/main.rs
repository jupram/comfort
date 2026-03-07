#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{
    CustomMenuItem, Manager, State, SystemTray, SystemTrayEvent, SystemTrayMenu, SystemTrayMenuItem,
};
use tracing::{error, info, warn};
use tracking_core::config::{AppSettings, CalibrationProfile};
use tracking_core::engine::{RuntimeEngine, WarmupState};
use tracking_core::input::SafeInputDriver;
use tracking_core::types::{
    CameraPreview, ControlIntent, GestureHint, RuntimeEvent, TrackingFrame, TrackingStatus,
    VisionStatus,
};

#[derive(Clone)]
enum RuntimeCommand {
    Start,
    Stop,
    Pause,
    Resume,
    UpdateSettings(AppSettings),
}

struct AppState {
    tx: Mutex<Sender<RuntimeCommand>>,
    settings: Mutex<AppSettings>,
    settings_path: PathBuf,
}

impl AppState {
    fn send(&self, cmd: RuntimeCommand) -> Result<(), String> {
        self.tx
            .lock()
            .map_err(|_| "runtime channel lock poisoned".to_string())?
            .send(cmd)
            .map_err(|e| format!("runtime command send failed: {e}"))
    }
}

struct EventDumpWriter {
    writer: BufWriter<File>,
}

impl EventDumpWriter {
    fn new() -> Option<Self> {
        let path = event_dump_path();
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                error!("event dump dir create failed: {e}");
                return None;
            }
        }
        match File::create(&path) {
            Ok(file) => {
                info!("runtime event dump enabled: {}", path.display());
                Some(Self {
                    writer: BufWriter::new(file),
                })
            }
            Err(e) => {
                error!("event dump file create failed ({}): {e}", path.display());
                None
            }
        }
    }

    fn write_payload<T: Serialize>(&mut self, event: &str, payload: &T) {
        let line = json!({
            "ts_ms": now_ms(),
            "event": event,
            "payload": payload
        });
        self.write_line(line);
    }

    fn write_line(&mut self, line: serde_json::Value) {
        match serde_json::to_string(&line) {
            Ok(s) => {
                if self.writer.write_all(s.as_bytes()).is_err()
                    || self.writer.write_all(b"\n").is_err()
                    || self.writer.flush().is_err()
                {
                    error!("event dump write failed");
                }
            }
            Err(e) => {
                error!("event dump serialization failed: {e}");
            }
        }
    }
}

#[derive(Debug, Serialize)]
struct CalibrationResult {
    profile: CalibrationProfile,
    pointer_range: f32,
    move_gain: f32,
    move_accel: f32,
    move_max_delta: f32,
    deadzone: f32,
    hold_to_control_ms: u64,
    clutch_enter_ms: u64,
    pinch_threshold: f32,
    right_pinch_threshold: f32,
    click_cooldown_ms: u64,
    confidence_lock: f32,
    confidence_unlock: f32,
    calibrated_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct CalibrationRequest {
    profile: CalibrationProfile,
    pointer_range: Option<f32>,
    move_gain: Option<f32>,
    move_accel: Option<f32>,
    move_max_delta: Option<f32>,
    deadzone: Option<f32>,
    hold_to_control_ms: Option<u64>,
    clutch_enter_ms: Option<u64>,
    pinch_threshold: Option<f32>,
    right_pinch_threshold: Option<f32>,
    click_cooldown_ms: Option<u64>,
    confidence_lock: Option<f32>,
    confidence_unlock: Option<f32>,
    input_injection_enabled: Option<bool>,
    safe_mode: Option<bool>,
    allow_safe_mode_movement: Option<bool>,
}

impl Default for CalibrationRequest {
    fn default() -> Self {
        Self {
            profile: CalibrationProfile::Balanced,
            pointer_range: None,
            move_gain: None,
            move_accel: None,
            move_max_delta: None,
            deadzone: None,
            hold_to_control_ms: None,
            clutch_enter_ms: None,
            pinch_threshold: None,
            right_pinch_threshold: None,
            click_cooldown_ms: None,
            confidence_lock: None,
            confidence_unlock: None,
            input_injection_enabled: None,
            safe_mode: None,
            allow_safe_mode_movement: None,
        }
    }
}

#[tauri::command]
fn start_tracking(state: State<AppState>) -> Result<(), String> {
    state.send(RuntimeCommand::Start)
}

#[tauri::command]
fn stop_tracking(state: State<AppState>) -> Result<(), String> {
    state.send(RuntimeCommand::Stop)
}

#[tauri::command]
fn pause_tracking(state: State<AppState>) -> Result<(), String> {
    state.send(RuntimeCommand::Pause)
}

#[tauri::command]
fn resume_tracking(state: State<AppState>) -> Result<(), String> {
    state.send(RuntimeCommand::Resume)
}

#[tauri::command]
fn load_settings(state: State<AppState>) -> Result<AppSettings, String> {
    state
        .settings
        .lock()
        .map(|cfg| cfg.clone())
        .map_err(|_| "settings lock poisoned".to_string())
}

#[tauri::command]
fn save_settings(state: State<AppState>, settings: AppSettings) -> Result<(), String> {
    let settings = settings.normalized();
    settings
        .save(&state.settings_path)
        .map_err(|e| format!("save settings failed: {e}"))?;
    {
        let mut guard = state
            .settings
            .lock()
            .map_err(|_| "settings lock poisoned".to_string())?;
        *guard = settings.clone();
    }
    state.send(RuntimeCommand::UpdateSettings(settings))
}

#[tauri::command]
fn reset_settings(state: State<AppState>) -> Result<AppSettings, String> {
    let cfg = AppSettings::default().normalized();
    cfg.save(&state.settings_path)
        .map_err(|e| format!("reset settings failed: {e}"))?;
    {
        let mut guard = state
            .settings
            .lock()
            .map_err(|_| "settings lock poisoned".to_string())?;
        *guard = cfg.clone();
    }
    state.send(RuntimeCommand::UpdateSettings(cfg.clone()))?;
    Ok(cfg)
}

#[tauri::command]
fn run_calibration(
    state: State<AppState>,
    request: Option<CalibrationRequest>,
) -> Result<CalibrationResult, String> {
    let req = request.unwrap_or_default();
    let mut cfg = state
        .settings
        .lock()
        .map_err(|_| "settings lock poisoned".to_string())?
        .clone();
    cfg.apply_calibration_profile(req.profile);
    if let Some(v) = req.pointer_range {
        cfg.pointer_range = v.clamp(0.5, 3.0);
    }
    if let Some(v) = req.move_gain {
        cfg.move_gain = v.clamp(1.0, 15.0);
    }
    if let Some(v) = req.move_accel {
        cfg.move_accel = v.clamp(0.0, 30.0);
    }
    if let Some(v) = req.move_max_delta {
        cfg.move_max_delta = v.clamp(0.01, 0.25);
    }
    if let Some(v) = req.deadzone {
        cfg.deadzone = v.clamp(0.001, 0.08);
    }
    if let Some(v) = req.hold_to_control_ms {
        cfg.hold_to_control_ms = v.clamp(20, 300);
    }
    if let Some(v) = req.clutch_enter_ms {
        cfg.clutch_enter_ms = v.clamp(20, 300);
    }
    if let Some(v) = req.pinch_threshold {
        cfg.pinch_threshold = v.clamp(0.015, 0.090);
    }
    if let Some(v) = req.right_pinch_threshold {
        cfg.right_pinch_threshold = v.clamp(0.015, 0.100);
    }
    if let Some(v) = req.click_cooldown_ms {
        cfg.click_cooldown_ms = v.clamp(60, 450);
    }
    if let Some(v) = req.confidence_lock {
        cfg.confidence_lock = v.clamp(0.35, 0.95);
    }
    if let Some(v) = req.confidence_unlock {
        cfg.confidence_unlock = v.clamp(0.20, 0.90);
    }
    cfg.normalize_in_place();
    if let Some(v) = req.input_injection_enabled {
        cfg.input_injection_enabled = v;
    }
    if let Some(v) = req.safe_mode {
        cfg.safe_mode = v;
    }
    if let Some(v) = req.allow_safe_mode_movement {
        cfg.allow_safe_mode_movement = v;
    }
    cfg.calibrated_at_ms = Some(now_ms());

    cfg.save(&state.settings_path)
        .map_err(|e| format!("save calibration failed: {e}"))?;
    {
        let mut guard = state
            .settings
            .lock()
            .map_err(|_| "settings lock poisoned".to_string())?;
        *guard = cfg.clone();
    }
    state.send(RuntimeCommand::UpdateSettings(cfg.clone()))?;
    Ok(CalibrationResult {
        profile: cfg.calibration_profile,
        pointer_range: cfg.pointer_range,
        move_gain: cfg.move_gain,
        move_accel: cfg.move_accel,
        move_max_delta: cfg.move_max_delta,
        deadzone: cfg.deadzone,
        hold_to_control_ms: cfg.hold_to_control_ms,
        clutch_enter_ms: cfg.clutch_enter_ms,
        pinch_threshold: cfg.pinch_threshold,
        right_pinch_threshold: cfg.right_pinch_threshold,
        click_cooldown_ms: cfg.click_cooldown_ms,
        confidence_lock: cfg.confidence_lock,
        confidence_unlock: cfg.confidence_unlock,
        calibrated_at_ms: cfg.calibrated_at_ms,
    })
}

fn main() {
    if let Err(e) = init_logging() {
        eprintln!("logging init failed: {e}");
    }

    tauri::Builder::default()
        .setup(|app| {
            let settings_path = default_settings_path();
            let existed = settings_path.exists();
            let loaded = AppSettings::load_or_default(&settings_path)?;
            let settings = loaded.clone().normalized();
            if !existed || settings != loaded {
                settings.save(&settings_path)?;
            }
            let tx = spawn_runtime_thread(app.handle(), settings.clone());
            app.manage(AppState {
                tx: Mutex::new(tx),
                settings: Mutex::new(settings),
                settings_path,
            });
            info!("app setup complete");
            Ok(())
        })
        .system_tray(build_tray())
        .on_system_tray_event(handle_system_tray_event)
        .invoke_handler(tauri::generate_handler![
            start_tracking,
            stop_tracking,
            pause_tracking,
            resume_tracking,
            load_settings,
            save_settings,
            reset_settings,
            run_calibration
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn build_tray() -> SystemTray {
    let start = CustomMenuItem::new("start".to_string(), "Start");
    let stop = CustomMenuItem::new("stop".to_string(), "Stop");
    let pause = CustomMenuItem::new("pause".to_string(), "Pause");
    let resume = CustomMenuItem::new("resume".to_string(), "Resume");
    let open = CustomMenuItem::new("open".to_string(), "Open Settings");
    let quit = CustomMenuItem::new("quit".to_string(), "Quit");

    let menu = SystemTrayMenu::new()
        .add_item(start)
        .add_item(stop)
        .add_item(pause)
        .add_item(resume)
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(open)
        .add_item(quit);

    SystemTray::new().with_menu(menu)
}

fn handle_system_tray_event(app: &tauri::AppHandle, event: SystemTrayEvent) {
    if let SystemTrayEvent::MenuItemClick { id, .. } = event {
        let state = app.state::<AppState>();
        match id.as_str() {
            "start" => {
                let _ = state.send(RuntimeCommand::Start);
            }
            "stop" => {
                let _ = state.send(RuntimeCommand::Stop);
            }
            "pause" => {
                let _ = state.send(RuntimeCommand::Pause);
            }
            "resume" => {
                let _ = state.send(RuntimeCommand::Resume);
            }
            "open" => {
                if let Some(w) = app.get_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        }
    }
}

fn spawn_runtime_thread(app: tauri::AppHandle, initial: AppSettings) -> Sender<RuntimeCommand> {
    let (tx, rx): (Sender<RuntimeCommand>, Receiver<RuntimeCommand>) = mpsc::channel();
    thread::spawn(move || {
        let mut event_dump = EventDumpWriter::new();
        let mut settings = initial;
        let mut engine = RuntimeEngine::new(settings.clone());
        let mut input = SafeInputDriver::new(settings.clone());
        let mut running = false;
        let mut paused = false;
        let mut vision_ready = false;
        let mut ready_announced = false;

        loop {
            while let Ok(cmd) = rx.try_recv() {
                match cmd {
                    RuntimeCommand::Start => {
                        running = true;
                        paused = false;
                        if let Some(dump) = event_dump.as_mut() {
                            dump.write_line(json!({
                                "ts_ms": now_ms(),
                                "event": "runtime_start",
                                "payload": { "running": true, "paused": false }
                            }));
                        }
                        emit_vision(&app, VisionStatus::Running);
                        if vision_ready {
                            emit_vision(&app, VisionStatus::Ready);
                            ready_announced = true;
                        } else {
                            emit_vision(&app, VisionStatus::LoadingModel);
                            emit_vision(&app, VisionStatus::LoadingCamera);
                        }
                        info!("runtime started");
                    }
                    RuntimeCommand::Stop => {
                        running = false;
                        paused = false;
                        ready_announced = false;
                        if let Some(dump) = event_dump.as_mut() {
                            dump.write_line(json!({
                                "ts_ms": now_ms(),
                                "event": "runtime_stop",
                                "payload": { "running": false, "paused": false }
                            }));
                        }
                        emit_tracking(&app, TrackingStatus::Lost);
                        emit_intent(&app, ControlIntent::ControlOff);
                        emit_vision(&app, VisionStatus::Stopped);
                        info!("runtime stopped");
                    }
                    RuntimeCommand::Pause => {
                        paused = true;
                        if let Some(dump) = event_dump.as_mut() {
                            dump.write_line(json!({
                                "ts_ms": now_ms(),
                                "event": "runtime_pause",
                                "payload": { "paused": true }
                            }));
                        }
                        emit_intent(&app, ControlIntent::Paused);
                        emit_vision(&app, VisionStatus::Paused);
                        info!("runtime paused");
                    }
                    RuntimeCommand::Resume => {
                        paused = false;
                        if let Some(dump) = event_dump.as_mut() {
                            dump.write_line(json!({
                                "ts_ms": now_ms(),
                                "event": "runtime_resume",
                                "payload": { "paused": false }
                            }));
                        }
                        emit_vision(&app, VisionStatus::Running);
                        if vision_ready && !ready_announced {
                            emit_vision(&app, VisionStatus::Ready);
                            ready_announced = true;
                        }
                        info!("runtime resumed");
                    }
                    RuntimeCommand::UpdateSettings(next) => {
                        settings = next;
                        engine.update_settings(settings.clone());
                        input.update_settings(settings.clone());
                        if let Some(dump) = event_dump.as_mut() {
                            dump.write_payload("settings_updated", &settings);
                        }
                        vision_ready = false;
                        ready_announced = false;
                        if running {
                            emit_vision(&app, VisionStatus::LoadingModel);
                            emit_vision(&app, VisionStatus::LoadingCamera);
                        }
                        info!("runtime settings updated");
                    }
                }
            }

            let ts_ms = now_ms();

            if !running {
                match engine.warmup_tick(ts_ms) {
                    WarmupState::NoFrame => {}
                    WarmupState::FrameAvailable | WarmupState::HandVisible => {
                        vision_ready = true;
                    }
                }
            }

            if running {
                for event in engine.tick(ts_ms, paused) {
                    match event {
                        RuntimeEvent::VisionStatus { status } => {
                            if let Some(dump) = event_dump.as_mut() {
                                dump.write_payload("vision_status", &status);
                            }
                            emit_vision(&app, status)
                        }
                        RuntimeEvent::TrackingStatus { status } => {
                            if let Some(dump) = event_dump.as_mut() {
                                dump.write_payload("tracking_status", &status);
                            }
                            emit_tracking(&app, status)
                        }
                        RuntimeEvent::TrackingFrame { frame } => {
                            if let Some(dump) = event_dump.as_mut() {
                                dump.write_line(json!({
                                    "ts_ms": now_ms(),
                                    "event": "tracking_frame",
                                    "payload": {
                                        "ts_ms": frame.ts_ms,
                                        "frame_id": frame.frame_id,
                                        "confidence": frame.confidence,
                                        "landmarks_len": frame.landmarks.len()
                                    }
                                }));
                            }
                            vision_ready = true;
                            if !ready_announced {
                                emit_vision(&app, VisionStatus::Ready);
                                ready_announced = true;
                            }
                            emit_frame(&app, frame)
                        }
                        RuntimeEvent::CameraPreview { frame } => {
                            if let Some(dump) = event_dump.as_mut() {
                                dump.write_line(json!({
                                    "ts_ms": now_ms(),
                                    "event": "camera_preview",
                                    "payload": {
                                        "ts_ms": frame.ts_ms,
                                        "frame_id": frame.frame_id,
                                        "jpeg_len": frame.jpeg_base64.len()
                                    }
                                }));
                            }
                            emit_camera_preview(&app, frame)
                        }
                        RuntimeEvent::GestureHint { hint } => {
                            if let Some(dump) = event_dump.as_mut() {
                                dump.write_payload("gesture_hint", &hint);
                            }
                            emit_gesture_hint(&app, hint)
                        }
                        RuntimeEvent::ControlIntent { intent } => {
                            if let Some(dump) = event_dump.as_mut() {
                                dump.write_payload("gesture_debug", &intent);
                            }
                            if let Err(e) = input.apply(&intent) {
                                warn!("input apply failed: {e}");
                            }
                            emit_intent(&app, intent);
                        }
                        RuntimeEvent::HealthMetrics { metrics } => {
                            if let Some(dump) = event_dump.as_mut() {
                                dump.write_payload("health_metrics", &metrics);
                            }
                            if let Err(e) = app.emit_all("health_metrics", metrics) {
                                error!("health event emit failed: {e}");
                            }
                        }
                    }
                }
            }

            let loop_ms = (1000 / settings.camera_fps.max(1)) as u64;
            thread::sleep(Duration::from_millis(loop_ms));
        }
    });
    tx
}

fn emit_tracking(app: &tauri::AppHandle, status: TrackingStatus) {
    if let Err(e) = app.emit_all("tracking_status", status) {
        error!("tracking event emit failed: {e}");
    }
}

fn emit_intent(app: &tauri::AppHandle, intent: ControlIntent) {
    if let Err(e) = app.emit_all("gesture_debug", intent) {
        error!("gesture_debug emit failed: {e}");
    }
}

fn emit_frame(app: &tauri::AppHandle, frame: TrackingFrame) {
    if let Err(e) = app.emit_all("tracking_frame", frame) {
        error!("tracking_frame emit failed: {e}");
    }
}

fn emit_vision(app: &tauri::AppHandle, status: VisionStatus) {
    if let Err(e) = app.emit_all("vision_status", status) {
        error!("vision_status emit failed: {e}");
    }
}

fn emit_camera_preview(app: &tauri::AppHandle, frame: CameraPreview) {
    if let Err(e) = app.emit_all("camera_preview", frame) {
        error!("camera_preview emit failed: {e}");
    }
}

fn emit_gesture_hint(app: &tauri::AppHandle, hint: GestureHint) {
    if let Err(e) = app.emit_all("gesture_hint", hint) {
        error!("gesture_hint emit failed: {e}");
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn default_settings_path() -> PathBuf {
    let mut root = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    root.push("gesture-mouse");
    root.push("settings.json");
    root
}

fn event_dump_path() -> PathBuf {
    let mut root = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    root.push("gesture-mouse");
    root.push("runtime-events.jsonl");
    root
}

fn init_logging() -> anyhow::Result<()> {
    let mut root = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    root.push("gesture-mouse");
    std::fs::create_dir_all(&root)?;
    let file_appender = tracing_appender::rolling::daily(root, "app.log");
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tauri=warn,wry=warn".into()),
        )
        .with_writer(file_appender)
        .with_ansi(false)
        .try_init()
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(())
}
