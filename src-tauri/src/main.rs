#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{
    CustomMenuItem, Manager, State, SystemTray, SystemTrayEvent, SystemTrayMenu, SystemTrayMenuItem,
};
use tracing::{error, info, warn};
use tracking_core::config::{AppSettings, CalibrationProfile};
use tracking_core::engine::RuntimeEngine;
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

fn coalesce_runtime_command(
    rx: &Receiver<RuntimeCommand>,
    cmd: RuntimeCommand,
) -> (RuntimeCommand, Option<RuntimeCommand>, usize) {
    let RuntimeCommand::UpdateSettings(mut latest) = cmd else {
        return (cmd, None, 0);
    };

    let mut coalesced = 0;
    while let Ok(next) = rx.try_recv() {
        match next {
            RuntimeCommand::UpdateSettings(settings) => {
                latest = settings;
                coalesced += 1;
            }
            other => {
                return (
                    RuntimeCommand::UpdateSettings(latest),
                    Some(other),
                    coalesced,
                )
            }
        }
    }

    (RuntimeCommand::UpdateSettings(latest), None, coalesced)
}

fn runtime_frame_period(camera_fps: u32) -> Duration {
    Duration::from_nanos(1_000_000_000 / camera_fps.max(1) as u64)
}

fn runtime_sleep_duration(camera_fps: u32, elapsed: Duration) -> Duration {
    runtime_frame_period(camera_fps).saturating_sub(elapsed)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct RuntimeSessionState {
    running: bool,
    paused: bool,
    vision_ready: bool,
    ready_announced: bool,
}

impl RuntimeSessionState {
    fn start(&mut self) -> StartDecision {
        self.running = true;
        self.paused = false;
        if self.vision_ready {
            self.ready_announced = true;
            StartDecision::AlreadyReady
        } else {
            StartDecision::NeedsLoading
        }
    }

    fn stop(&mut self) {
        self.running = false;
        self.paused = false;
        self.vision_ready = false;
        self.ready_announced = false;
    }

    fn pause(&mut self) -> bool {
        if !self.running {
            return false;
        }
        self.paused = true;
        true
    }

    fn resume(&mut self) -> ResumeDecision {
        if !self.running {
            return ResumeDecision {
                accepted: false,
                announce_ready: false,
            };
        }
        self.paused = false;
        let announce_ready = self.vision_ready && !self.ready_announced;
        if announce_ready {
            self.ready_announced = true;
        }
        ResumeDecision {
            accepted: true,
            announce_ready,
        }
    }

    fn apply_tracker_settings_change(&mut self, tracker_changed: bool) -> bool {
        if tracker_changed {
            self.vision_ready = false;
            self.ready_announced = false;
        }
        self.running && tracker_changed
    }

    fn mark_vision_ready(&mut self) -> bool {
        self.vision_ready = true;
        if self.ready_announced {
            return false;
        }
        self.ready_announced = true;
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartDecision {
    AlreadyReady,
    NeedsLoading,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResumeDecision {
    accepted: bool,
    announce_ready: bool,
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
    pending_lines: usize,
}

impl EventDumpWriter {
    const FLUSH_EVERY_LINES: usize = 32;

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
                    pending_lines: 0,
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
                {
                    error!("event dump write failed");
                    return;
                }
                self.pending_lines += 1;
                if self.pending_lines >= Self::FLUSH_EVERY_LINES {
                    self.flush();
                }
            }
            Err(e) => {
                error!("event dump serialization failed: {e}");
            }
        }
    }

    fn flush(&mut self) {
        if self.pending_lines == 0 {
            return;
        }
        if self.writer.flush().is_err() {
            error!("event dump flush failed");
        }
        self.pending_lines = 0;
    }
}

impl Drop for EventDumpWriter {
    fn drop(&mut self) {
        self.flush();
    }
}

fn event_dump_mut<'a>(
    event_dump: &'a mut Option<EventDumpWriter>,
    settings: &AppSettings,
) -> Option<&'a mut EventDumpWriter> {
    if !settings.diagnostics_enabled {
        return None;
    }
    if event_dump.is_none() {
        *event_dump = EventDumpWriter::new();
    }
    event_dump.as_mut()
}

fn disable_event_dump_writer(event_dump: &mut Option<EventDumpWriter>) {
    if event_dump.take().is_some() {
        info!("runtime event dump disabled");
    }
}

fn write_event_dump_payload<T: Serialize>(
    event_dump: &mut Option<EventDumpWriter>,
    settings: &AppSettings,
    event: &str,
    payload: &T,
) {
    if let Some(dump) = event_dump_mut(event_dump, settings) {
        dump.write_payload(event, payload);
    }
}

fn write_event_dump_line_lazy<F>(
    event_dump: &mut Option<EventDumpWriter>,
    settings: &AppSettings,
    build_line: F,
) where
    F: FnOnce() -> serde_json::Value,
{
    if let Some(dump) = event_dump_mut(event_dump, settings) {
        dump.write_line(build_line());
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

impl From<&AppSettings> for CalibrationResult {
    fn from(cfg: &AppSettings) -> Self {
        Self {
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
        }
    }
}

fn current_settings(state: &AppState) -> Result<AppSettings, String> {
    state
        .settings
        .lock()
        .map(|cfg| cfg.clone())
        .map_err(|_| "settings lock poisoned".to_string())
}

fn persist_settings(
    state: &AppState,
    settings: AppSettings,
    error_context: &str,
) -> Result<AppSettings, String> {
    let settings = settings.normalized();
    settings
        .save(&state.settings_path)
        .map_err(|e| format!("{error_context}: {e}"))?;
    {
        let mut guard = state
            .settings
            .lock()
            .map_err(|_| "settings lock poisoned".to_string())?;
        *guard = settings.clone();
    }
    state.send(RuntimeCommand::UpdateSettings(settings.clone()))?;
    Ok(settings)
}

fn build_calibrated_settings(
    mut cfg: AppSettings,
    req: CalibrationRequest,
    calibrated_at_ms: u64,
) -> AppSettings {
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
    cfg.calibrated_at_ms = Some(calibrated_at_ms);
    cfg
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
    current_settings(&state)
}

#[tauri::command]
fn save_settings(state: State<AppState>, settings: AppSettings) -> Result<(), String> {
    persist_settings(&state, settings, "save settings failed").map(|_| ())
}

#[tauri::command]
fn reset_settings(state: State<AppState>) -> Result<AppSettings, String> {
    persist_settings(
        &state,
        AppSettings::default().normalized(),
        "reset settings failed",
    )
}

#[tauri::command]
fn run_calibration(
    state: State<AppState>,
    request: Option<CalibrationRequest>,
) -> Result<CalibrationResult, String> {
    let req = request.unwrap_or_default();
    let cfg = build_calibrated_settings(current_settings(&state)?, req, now_ms());
    persist_settings(&state, cfg, "save calibration failed")
        .map(|cfg| CalibrationResult::from(&cfg))
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
        let mut settings = initial;
        let mut event_dump = None;
        let mut engine = RuntimeEngine::new(settings.clone());
        let mut input = SafeInputDriver::new(&settings);
        let mut session = RuntimeSessionState::default();
        let mut pending_cmd = None;
        let mut runtime_events = Vec::with_capacity(4);

        loop {
            let frame_started = Instant::now();
            while let Some(raw_cmd) = pending_cmd.take().or_else(|| rx.try_recv().ok()) {
                let (cmd, next_pending, coalesced_settings) =
                    coalesce_runtime_command(&rx, raw_cmd);
                pending_cmd = next_pending;
                if coalesced_settings > 0 {
                    info!("coalesced {coalesced_settings} queued runtime settings update(s)");
                }
                match cmd {
                    RuntimeCommand::Start => {
                        let start = session.start();
                        write_event_dump_line_lazy(&mut event_dump, &settings, || {
                            json!({
                                "ts_ms": now_ms(),
                                "event": "runtime_start",
                                "payload": { "running": true, "paused": false }
                            })
                        });
                        emit_vision(&app, VisionStatus::Running);
                        match start {
                            StartDecision::AlreadyReady => emit_vision(&app, VisionStatus::Ready),
                            StartDecision::NeedsLoading => {
                                emit_vision(&app, VisionStatus::LoadingModel);
                                emit_vision(&app, VisionStatus::LoadingCamera);
                            }
                        }
                        info!("runtime started");
                    }
                    RuntimeCommand::Stop => {
                        session.stop();
                        let _ = engine.stop(now_ms());
                        write_event_dump_line_lazy(&mut event_dump, &settings, || {
                            json!({
                                "ts_ms": now_ms(),
                                "event": "runtime_stop",
                                "payload": { "running": false, "paused": false }
                            })
                        });
                        emit_tracking(&app, TrackingStatus::Lost);
                        emit_intent(&app, ControlIntent::ControlOff);
                        emit_vision(&app, VisionStatus::Stopped);
                        info!("runtime stopped");
                    }
                    RuntimeCommand::Pause => {
                        if !session.pause() {
                            info!("pause ignored because runtime is stopped");
                            continue;
                        }
                        write_event_dump_line_lazy(&mut event_dump, &settings, || {
                            json!({
                                "ts_ms": now_ms(),
                                "event": "runtime_pause",
                                "payload": { "paused": true }
                            })
                        });
                        emit_intent(&app, ControlIntent::Paused);
                        emit_vision(&app, VisionStatus::Paused);
                        info!("runtime paused");
                    }
                    RuntimeCommand::Resume => {
                        let resume = session.resume();
                        if !resume.accepted {
                            info!("resume ignored because runtime is stopped");
                            continue;
                        }
                        write_event_dump_line_lazy(&mut event_dump, &settings, || {
                            json!({
                                "ts_ms": now_ms(),
                                "event": "runtime_resume",
                                "payload": { "paused": false }
                            })
                        });
                        emit_vision(&app, VisionStatus::Running);
                        if resume.announce_ready {
                            emit_vision(&app, VisionStatus::Ready);
                        }
                        info!("runtime resumed");
                    }
                    RuntimeCommand::UpdateSettings(next) => {
                        if !next.diagnostics_enabled {
                            disable_event_dump_writer(&mut event_dump);
                        }
                        let tracker_changed = engine.update_settings(next.clone());
                        input.update_settings(&next);
                        write_event_dump_payload(&mut event_dump, &next, "settings_updated", &next);
                        settings = next;
                        if session.apply_tracker_settings_change(tracker_changed) {
                            emit_vision(&app, VisionStatus::LoadingModel);
                            emit_vision(&app, VisionStatus::LoadingCamera);
                        }
                        info!("runtime settings updated");
                    }
                }
            }

            let ts_ms = now_ms();

            if session.running {
                engine.tick_into(ts_ms, session.paused, &mut runtime_events);
                for event in runtime_events.drain(..) {
                    handle_runtime_event(
                        &app,
                        &settings,
                        &mut event_dump,
                        &mut input,
                        &mut session,
                        event,
                    );
                }
            }

            if session.running {
                let sleep_duration =
                    runtime_sleep_duration(settings.camera_fps, frame_started.elapsed());
                if !sleep_duration.is_zero() {
                    thread::sleep(sleep_duration);
                }
            } else {
                match rx.recv() {
                    Ok(cmd) => pending_cmd = Some(cmd),
                    Err(_) => break,
                }
            }
        }
    });
    tx
}

fn handle_runtime_event(
    app: &tauri::AppHandle,
    settings: &AppSettings,
    event_dump: &mut Option<EventDumpWriter>,
    input: &mut SafeInputDriver,
    session: &mut RuntimeSessionState,
    event: RuntimeEvent,
) {
    match event {
        RuntimeEvent::VisionStatus { status } => {
            write_event_dump_payload(event_dump, settings, "vision_status", &status);
            emit_vision(app, status)
        }
        RuntimeEvent::TrackingStatus { status } => {
            write_event_dump_payload(event_dump, settings, "tracking_status", &status);
            emit_tracking(app, status)
        }
        RuntimeEvent::TrackingFrame { frame } => {
            write_event_dump_line_lazy(event_dump, settings, || {
                json!({
                    "ts_ms": now_ms(),
                    "event": "tracking_frame",
                    "payload": {
                        "ts_ms": frame.ts_ms,
                        "frame_id": frame.frame_id,
                        "confidence": frame.confidence,
                        "landmarks_len": frame.landmarks.len()
                    }
                })
            });
            if session.mark_vision_ready() {
                emit_vision(app, VisionStatus::Ready);
            }
            emit_frame(app, frame)
        }
        RuntimeEvent::CameraPreview { frame } => {
            write_event_dump_line_lazy(event_dump, settings, || {
                json!({
                    "ts_ms": now_ms(),
                    "event": "camera_preview",
                    "payload": {
                        "ts_ms": frame.ts_ms,
                        "frame_id": frame.frame_id,
                        "jpeg_len": frame.jpeg_base64.len()
                    }
                })
            });
            if session.mark_vision_ready() {
                emit_vision(app, VisionStatus::Ready);
            }
            emit_camera_preview(app, frame)
        }
        RuntimeEvent::GestureHint { hint } => {
            write_event_dump_payload(event_dump, settings, "gesture_hint", &hint);
            emit_gesture_hint(app, hint)
        }
        RuntimeEvent::ControlIntent { intent } => {
            write_event_dump_payload(event_dump, settings, "gesture_debug", &intent);
            if let Err(e) = input.apply(&intent) {
                warn!("input apply failed: {e}");
            }
            emit_intent(app, intent);
        }
        RuntimeEvent::HealthMetrics { metrics } => {
            write_event_dump_payload(event_dump, settings, "health_metrics", &metrics);
            if let Err(e) = app.emit_all("health_metrics", metrics) {
                error!("health event emit failed: {e}");
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_session_start_selects_loading_until_vision_is_ready() {
        let mut session = RuntimeSessionState::default();

        assert_eq!(session.start(), StartDecision::NeedsLoading);
        assert!(session.running);
        assert!(!session.paused);
        assert!(!session.ready_announced);

        session.stop();
        session.vision_ready = true;

        assert_eq!(session.start(), StartDecision::AlreadyReady);
        assert!(session.ready_announced);
    }

    #[test]
    fn runtime_session_ignores_pause_and_resume_while_stopped() {
        let mut session = RuntimeSessionState::default();

        assert!(!session.pause());
        assert_eq!(
            session.resume(),
            ResumeDecision {
                accepted: false,
                announce_ready: false,
            }
        );
        assert!(!session.running);
        assert!(!session.paused);
    }

    #[test]
    fn runtime_session_marks_vision_ready_once_per_tracker_generation() {
        let mut session = RuntimeSessionState::default();
        let _ = session.start();

        assert!(session.mark_vision_ready());
        assert!(!session.mark_vision_ready());

        assert!(session.apply_tracker_settings_change(true));
        assert!(!session.vision_ready);
        assert!(!session.ready_announced);
        assert!(session.mark_vision_ready());
    }

    #[test]
    fn runtime_session_settings_emit_loading_only_for_running_tracker_changes() {
        let mut session = RuntimeSessionState::default();

        assert!(!session.apply_tracker_settings_change(true));

        let _ = session.start();
        session.vision_ready = true;
        session.ready_announced = true;

        assert!(!session.apply_tracker_settings_change(false));
        assert!(session.vision_ready);
        assert!(session.ready_announced);

        assert!(session.apply_tracker_settings_change(true));
        assert!(!session.vision_ready);
        assert!(!session.ready_announced);
    }

    #[test]
    fn coalesces_consecutive_runtime_settings_updates() {
        let (tx, rx) = mpsc::channel();
        let first = AppSettings {
            camera_width: 640,
            ..AppSettings::default()
        };
        let second = AppSettings {
            camera_width: 800,
            ..AppSettings::default()
        };
        let latest = AppSettings {
            camera_width: 1024,
            ..AppSettings::default()
        };

        tx.send(RuntimeCommand::UpdateSettings(second))
            .expect("send second");
        tx.send(RuntimeCommand::UpdateSettings(latest.clone()))
            .expect("send latest");

        let (cmd, pending, coalesced) =
            coalesce_runtime_command(&rx, RuntimeCommand::UpdateSettings(first));

        assert_eq!(coalesced, 2);
        assert!(pending.is_none());
        match cmd {
            RuntimeCommand::UpdateSettings(settings) => assert_eq!(settings, latest),
            _ => panic!("expected settings update"),
        }
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn runtime_settings_coalescing_preserves_next_control_command() {
        let (tx, rx) = mpsc::channel();
        let first = AppSettings {
            camera_width: 640,
            ..AppSettings::default()
        };
        let latest_before_stop = AppSettings {
            camera_width: 800,
            ..AppSettings::default()
        };
        let later_settings = AppSettings {
            camera_width: 1024,
            ..AppSettings::default()
        };

        tx.send(RuntimeCommand::UpdateSettings(latest_before_stop.clone()))
            .expect("send settings");
        tx.send(RuntimeCommand::Stop).expect("send stop");
        tx.send(RuntimeCommand::UpdateSettings(later_settings.clone()))
            .expect("send later settings");

        let (cmd, pending, coalesced) =
            coalesce_runtime_command(&rx, RuntimeCommand::UpdateSettings(first));

        assert_eq!(coalesced, 1);
        match cmd {
            RuntimeCommand::UpdateSettings(settings) => assert_eq!(settings, latest_before_stop),
            _ => panic!("expected settings update"),
        }
        assert!(matches!(pending, Some(RuntimeCommand::Stop)));
        match rx.try_recv().expect("later command remains queued") {
            RuntimeCommand::UpdateSettings(settings) => assert_eq!(settings, later_settings),
            _ => panic!("expected later settings update"),
        }
    }

    #[test]
    fn runtime_frame_period_uses_configured_fps() {
        assert_eq!(runtime_frame_period(0), Duration::from_secs(1));
        assert_eq!(runtime_frame_period(1), Duration::from_secs(1));
        assert_eq!(runtime_frame_period(30), Duration::from_nanos(33_333_333));
        assert_eq!(runtime_frame_period(120), Duration::from_nanos(8_333_333));
    }

    #[test]
    fn runtime_sleep_duration_counts_elapsed_work() {
        assert_eq!(
            runtime_sleep_duration(30, Duration::from_millis(10)),
            Duration::from_nanos(23_333_333)
        );
        assert_eq!(
            runtime_sleep_duration(30, Duration::from_millis(40)),
            Duration::ZERO
        );
    }

    #[test]
    fn calibration_builder_clamps_values_and_preserves_safety_overrides() {
        let req = CalibrationRequest {
            profile: CalibrationProfile::Responsive,
            pointer_range: Some(99.0),
            move_gain: Some(0.0),
            move_accel: Some(99.0),
            move_max_delta: Some(0.0),
            deadzone: Some(1.0),
            hold_to_control_ms: Some(1),
            clutch_enter_ms: Some(1_000),
            pinch_threshold: Some(f32::NAN),
            right_pinch_threshold: Some(99.0),
            click_cooldown_ms: Some(1),
            confidence_lock: Some(f32::NAN),
            confidence_unlock: Some(0.99),
            input_injection_enabled: Some(true),
            safe_mode: Some(false),
            allow_safe_mode_movement: Some(true),
        };

        let cfg = build_calibrated_settings(AppSettings::default(), req, 123);

        assert_eq!(cfg.calibration_profile, CalibrationProfile::Responsive);
        assert_eq!(cfg.pointer_range, 3.0);
        assert_eq!(cfg.move_gain, 1.0);
        assert_eq!(cfg.move_accel, 30.0);
        assert_eq!(cfg.move_max_delta, 0.01);
        assert_eq!(cfg.deadzone, 0.08);
        assert_eq!(cfg.hold_to_control_ms, 20);
        assert_eq!(cfg.clutch_enter_ms, 300);
        assert_eq!(cfg.pinch_threshold, 0.035);
        assert_eq!(cfg.right_pinch_threshold, 0.100);
        assert_eq!(cfg.click_cooldown_ms, 60);
        assert_eq!(cfg.confidence_lock, 0.65);
        assert!(cfg.confidence_unlock < cfg.confidence_lock);
        assert!(cfg.input_injection_enabled);
        assert!(!cfg.safe_mode);
        assert!(cfg.allow_safe_mode_movement);
        assert_eq!(cfg.calibrated_at_ms, Some(123));
    }

    #[test]
    fn calibration_result_reflects_settings_snapshot() {
        let cfg = AppSettings {
            calibration_profile: CalibrationProfile::Comfort,
            calibrated_at_ms: Some(77),
            pointer_range: 1.25,
            ..AppSettings::default()
        };

        let result = CalibrationResult::from(&cfg);

        assert_eq!(result.profile, CalibrationProfile::Comfort);
        assert_eq!(result.pointer_range, 1.25);
        assert_eq!(result.calibrated_at_ms, Some(77));
    }

    #[test]
    fn event_dump_line_lazy_skips_builder_when_diagnostics_disabled() {
        let mut event_dump = None;
        let settings = AppSettings {
            diagnostics_enabled: false,
            ..AppSettings::default()
        };
        let mut called = false;

        write_event_dump_line_lazy(&mut event_dump, &settings, || {
            called = true;
            json!({ "event": "should_not_be_built" })
        });

        assert!(!called);
        assert!(event_dump.is_none());
    }
}
