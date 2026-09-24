use crate::audio::AudioManager;
use crate::config::AppConfig;
use crate::config::{ENGINE_ID_GENERIC, ENGINE_ID_VOICEGER, ENGINE_ID_VOICEVOX};
use crate::tts::types::{Speaker, SynthParams};
use crate::tts::voicevox::VoicevoxEngine;
use crate::tts::{EngineRegistry, TtsEngine};
use crate::ui::Screen;

use anyhow::Context as _;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Instant;

struct PlaybackGuard(Arc<AtomicBool>);
impl Drop for PlaybackGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Messages from the tokio runtime back to the UI thread
enum UiMessage {
    WavReady {
        wav: Vec<u8>,
        text: String,
    },
    SpeakersLoaded {
        engine_id: String,
        speakers: Vec<Speaker>,
    },
    HealthCheckResult {
        engine_id: String,
        ok: bool,
    },
    Error(String),
    UserDictLoaded(crate::tts::types::UserDict),
    UserDictUpdated,
    LoudnessScanComplete(
        std::collections::HashMap<std::path::PathBuf, crate::audio::loudness::LoudnessInfo>,
    ),
}

#[derive(Debug, Clone)]
pub enum EngineCommandConfig {
    Generic { url: String },
}

/// Commands from the UI thread to the tokio runtime
pub enum TtsCommand {
    Synthesize {
        text: String,
        params: SynthParams,
        engine_id: String,
        config: Option<EngineCommandConfig>,
    },
    LoadSpeakers {
        engine_id: String,
        config: Option<EngineCommandConfig>,
    },
    HealthCheck {
        engine_id: String,
        config: Option<EngineCommandConfig>,
    },
    LoadUserDict,
    AddUserDictWord {
        surface: String,
        pronunciation: String,
    },
    DeleteUserDictWord {
        uuid: String,
    },
}

/// Shared mutable state accessed by UI drawing functions
pub struct AppState {
    pub config: AppConfig,
    pub input_text: String,
    pub speakers: Vec<Speaker>,
    pub voicevox_connected: bool,
    pub voiceger_connected: bool,
    pub generic_connected: bool,
    pub generic_speakers: Vec<Speaker>,
    pub device_ready: bool,
    pub is_synthesizing: bool,
    pub is_playing: bool,
    pub last_error: Option<String>,
    pub pending_send: Option<String>,
    pub pending_health_check: bool,
    pub pending_health_check_generic: bool,
    pub pending_create_device: bool,
    pub pending_destroy_device: bool,
    pub pending_launch_voicevox: bool,
    pub pending_launch_voiceger: bool,
    pub pending_restart_voicevox: bool,
    pub pending_restart_voiceger: bool,
    pub voicevox_launching: bool,
    pub voiceger_launching: bool,
    pub current_screen: Screen,
    pub new_template_text: String,
    pub user_dict: Vec<(String, String, String)>, // (uuid, surface, pronunciation)
    pub new_dict_surface: String,
    pub new_dict_pronunciation: String,
    pub pending_load_user_dict: bool,
    pub pending_add_dict_word: Option<(String, String)>,
    pub pending_delete_dict_word: Option<String>,
    pub soundboard_files: Vec<(String, std::path::PathBuf)>,
    pub pending_soundboard_scan: bool,
    pub pending_soundboard_play: Option<std::path::PathBuf>,
    pub pending_soundboard_stop: bool,
    pub is_soundboard_playing: bool,
    pub sink_inputs: Vec<crate::media::desktop_capture::SinkInput>,
    pub pending_refresh_sink_inputs: bool,
    pub pending_start_capture: Option<(u32, String)>,
    pub pending_stop_capture: bool,
    pub is_capturing: bool,
    pub error_display_time: Option<std::time::Instant>,
    pub error_hovered: bool,
    pub templates_expanded: bool,
    pub adding_template: bool,
    pub needs_theme_update: bool,
    pub mic_passthrough: bool,
    pub pending_toggle_mic: bool,
    pub pending_reconnect_mic: bool,
    pub available_mic_sources: Vec<(String, String)>, // (name, description)
    pub pending_refresh_mic_sources: bool,
    pub color_edit_buffers: std::collections::HashMap<String, String>,
    pub pending_stop_speaking: bool,
    pub soundboard_loudness:
        std::collections::HashMap<std::path::PathBuf, crate::audio::loudness::LoudnessInfo>,
    pub pending_normalize_all: bool,
    pub pending_loudness_scan: bool,
    pub needs_initial_focus: bool,
    pub active_preset_idx: Option<usize>,
    pub preset_edit_idx: Option<usize>,
    pub preset_adding: bool,
    pub preset_edit_buf: Option<crate::config::SpeakerPreset>,
    /// Currently selected language in the Voiceger dictionary editor.
    pub voiceger_dict_lang: String,
    /// True when auto_launch_voiceger is enabled and we haven't yet received
    /// the first health check result. We defer the actual spawn until we know
    /// the server is not already running, to avoid "address already in use".
    pub voiceger_auto_launch_pending: bool,
    /// True when auto_launch_voicevox is enabled and we haven't yet received
    /// the first health check result. This avoids launching when URL-only
    /// operation is intended (empty voicevox_path).
    pub voicevox_auto_launch_pending: bool,
    /// Set by the UI when the user selects a different engine.
    /// process_actions() will handle health check, speaker reload, and preset switch.
    pub pending_switch_engine: Option<crate::config::TtsEngineType>,
}

const DOCKER_CONTAINER_NAME: &str = "zundux-voicevox";
const DOCKER_NAME_FILTER: &str = "name=zundux-voicevox";

pub struct ZunduxApp {
    state: AppState,
    audio_manager: AudioManager,
    ui_rx: mpsc::Receiver<UiMessage>,
    tts_tx: mpsc::Sender<TtsCommand>,
    voicevox_process: Option<Child>,
    voiceger_process: Option<Child>,
    is_docker: bool,
    last_health_check: Instant,
    pub is_playing: Arc<AtomicBool>,
    tts_cancel: Arc<AtomicBool>,
    audio_queue: std::collections::VecDeque<Vec<u8>>,
    is_soundboard_playing: Arc<AtomicBool>,
    soundboard_pids: Arc<std::sync::Mutex<Vec<u32>>>,
    soundboard_cancel: Arc<AtomicBool>,
    ui_tx: mpsc::Sender<UiMessage>,
    desktop_capture: crate::media::desktop_capture::DesktopCapture,
    needs_theme_update: bool,
    /// Echo settings shared with the mic effect pipeline (updated every frame).
    mic_echo_params: Arc<crate::audio::mic_effects::MicEchoParams>,
    /// Whether echo was requested when the mic passthrough was last (re)connected.
    mic_echo_requested: bool,
    /// Whether the virtual mic is currently set as the system default input by us.
    default_input_applied: bool,
}

const HEALTH_CHECK_INTERVAL_SECS: u64 = 5;
const HEALTH_CHECK_INTERVAL_LAUNCHING_SECS: u64 = 1;
const VOICEGER_LOG_PATH: &str = "/tmp/zundux_voiceger.log";

impl ZunduxApp {
    pub fn new(config: AppConfig, rt: tokio::runtime::Handle) -> Self {
        let (ui_tx, ui_rx) = mpsc::channel::<UiMessage>();
        let ui_tx_clone = ui_tx.clone();
        let (tts_tx, tts_rx) = mpsc::channel::<TtsCommand>();

        let auto_launch_voicevox = config.auto_launch_voicevox;
        let auto_launch_voiceger = config.auto_launch_voiceger;
        let templates_default_expanded = config.templates_default_expanded;
        let device_name = config.virtual_device_name.clone();
        let mut audio_manager = AudioManager::new(&device_name);
        let device_ready =
            audio_manager.ensure_device().is_ok() && audio_manager.device_exists().unwrap_or(false);

        crate::media::desktop_capture::DesktopCapture::cleanup_stale();
        audio_manager.virtual_device.cleanup_stale_loopbacks();
        let mic_echo_params = Arc::new(crate::audio::mic_effects::MicEchoParams::new(
            config.echo_delay_ms,
            config.echo_decay,
        ));

        // Spawn the TTS command processing loop on tokio
        let voicevox_url = config.voicevox_url.clone();
        let voiceger_url = config.voiceger_url.clone();
        let voiceger_ref_audio = config.voiceger_ref_audio.clone();
        let voiceger_prompt_text = config.voiceger_prompt_text.clone();
        let voiceger_prompt_lang = config.voiceger_prompt_lang.clone();
        let initial_generic_url = config
            .active_generic_config()
            .map(|e| e.url.clone())
            .unwrap_or_default();
        rt.spawn(Self::tts_loop(
            voicevox_url,
            voiceger_url,
            voiceger_ref_audio,
            voiceger_prompt_text,
            voiceger_prompt_lang,
            tts_rx,
            ui_tx,
        ));

        // Trigger initial health checks only — speaker lists will be loaded
        // when each engine's first health check succeeds (see HealthCheckResult handling).
        // Sending LoadSpeakers here would fail if the engine isn't running yet and
        // flash a misleading "スピーカー取得失敗" error.
        let _ = tts_tx.send(TtsCommand::HealthCheck {
            engine_id: ENGINE_ID_VOICEVOX.to_string(),
            config: None,
        });
        let _ = tts_tx.send(TtsCommand::HealthCheck {
            engine_id: ENGINE_ID_VOICEGER.to_string(),
            config: None,
        });
        let _ = tts_tx.send(TtsCommand::HealthCheck {
            engine_id: ENGINE_ID_GENERIC.to_string(),
            config: Some(EngineCommandConfig::Generic {
                url: initial_generic_url,
            }),
        });

        Self {
            state: AppState {
                config,
                input_text: String::new(),
                speakers: Vec::new(),
                voicevox_connected: false,
                voiceger_connected: false,
                generic_connected: false,
                generic_speakers: Vec::new(),
                device_ready,
                is_synthesizing: false,
                is_playing: false,
                last_error: None,
                pending_send: None,
                pending_health_check: false,
                pending_health_check_generic: false,
                pending_create_device: false,
                pending_destroy_device: false,
                pending_launch_voicevox: false,
                pending_launch_voiceger: false,
                pending_restart_voicevox: false,
                pending_restart_voiceger: false,
                voicevox_launching: false,
                voiceger_launching: false,
                current_screen: Screen::Input,
                new_template_text: String::new(),
                user_dict: Vec::new(),
                new_dict_surface: String::new(),
                new_dict_pronunciation: String::new(),
                pending_load_user_dict: false,
                pending_add_dict_word: None,
                pending_delete_dict_word: None,
                soundboard_files: Vec::new(),
                pending_soundboard_scan: true,
                pending_soundboard_play: None,
                pending_soundboard_stop: false,
                is_soundboard_playing: false,
                sink_inputs: Vec::new(),
                pending_refresh_sink_inputs: false,
                pending_start_capture: None,
                pending_stop_capture: false,
                is_capturing: false,
                error_display_time: None,
                error_hovered: false,
                templates_expanded: templates_default_expanded,
                adding_template: false,
                needs_theme_update: false,
                mic_passthrough: false,
                pending_toggle_mic: false,
                pending_reconnect_mic: false,
                available_mic_sources: Vec::new(),
                pending_refresh_mic_sources: true,
                color_edit_buffers: std::collections::HashMap::new(),
                pending_stop_speaking: false,
                soundboard_loudness: std::collections::HashMap::new(),
                pending_normalize_all: false,
                pending_loudness_scan: true,
                needs_initial_focus: true,
                active_preset_idx: None,
                preset_edit_idx: None,
                preset_adding: false,
                preset_edit_buf: None,
                voiceger_dict_lang: "ja".to_string(),
                voiceger_auto_launch_pending: auto_launch_voiceger,
                voicevox_auto_launch_pending: auto_launch_voicevox,
                pending_switch_engine: None,
            },
            audio_manager,
            ui_rx,
            tts_tx,
            voicevox_process: None,
            voiceger_process: None,
            is_docker: false,
            last_health_check: Instant::now(),
            is_playing: Arc::new(AtomicBool::new(false)),
            tts_cancel: Arc::new(AtomicBool::new(false)),
            audio_queue: std::collections::VecDeque::new(),
            is_soundboard_playing: Arc::new(AtomicBool::new(false)),
            soundboard_pids: Arc::new(std::sync::Mutex::new(Vec::new())),
            soundboard_cancel: Arc::new(AtomicBool::new(false)),
            ui_tx: ui_tx_clone,
            desktop_capture: crate::media::desktop_capture::DesktopCapture::new(),
            needs_theme_update: true,
            mic_echo_params,
            mic_echo_requested: false,
            default_input_applied: false,
        }
    }

    /// Echo params for the mic, or `None` when the mic should stay dry
    /// (echo off, or effects limited to the synthesized voice).
    fn mic_echo(&self) -> Option<Arc<crate::audio::mic_effects::MicEchoParams>> {
        let config = &self.state.config;
        (config.echo_enabled && !config.effects_tts_only).then(|| Arc::clone(&self.mic_echo_params))
    }

    /// The real microphone to pass through. When none is selected we must not use
    /// `@DEFAULT_SOURCE@` blindly: with "PCの既定の入力" ON it is our own virtual mic.
    fn mic_source(&self) -> String {
        let device = &self.audio_manager.virtual_device;
        self.state
            .config
            .mic_source_name
            .clone()
            .filter(|name| !device.is_own_source(name))
            .or_else(|| device.previous_default_source().map(str::to_string))
            .unwrap_or_else(|| "@DEFAULT_SOURCE@".to_string())
    }

    fn enable_mic_passthrough(&mut self, source: &str) -> anyhow::Result<()> {
        let echo = self.mic_echo();
        self.mic_echo_requested = echo.is_some();
        self.audio_manager.virtual_device.enable_mic_passthrough(
            source,
            self.state.config.noise_suppression,
            echo,
        )
    }

    /// Keep live audio settings in sync with the config each frame.
    fn sync_audio_settings(&mut self) {
        self.mic_echo_params.set(
            self.state.config.echo_delay_ms,
            self.state.config.echo_decay,
        );

        // Echo on/off or "TTS only" toggled while the mic is on → reconnect so the
        // mic switches between the plain loopback and the effect pipeline.
        if self.state.mic_passthrough && self.mic_echo().is_some() != self.mic_echo_requested {
            self.state.pending_reconnect_mic = true;
        }

        let want_default = self.state.config.use_as_default_input && self.state.device_ready;
        if want_default != self.default_input_applied {
            self.default_input_applied = want_default;
            if want_default {
                if let Err(e) = self.audio_manager.virtual_device.set_as_default_source() {
                    tracing::warn!("Failed to set default input: {}", e);
                    self.state.last_error = Some(format!("既定の入力デバイス設定失敗: {}", e));
                    // Don't retry every frame; the user can toggle again.
                    self.state.config.use_as_default_input = false;
                    self.default_input_applied = false;
                }
            } else {
                self.audio_manager.virtual_device.restore_default_source();
            }
        }
    }

    fn normalize_engine_id(engine_id: &str) -> String {
        let trimmed = engine_id.trim();
        if let Some(engine) = crate::config::TtsEngineType::from_engine_id(trimmed) {
            engine.as_engine_id().to_string()
        } else {
            trimmed.to_ascii_lowercase()
        }
    }

    fn preset_engine_id(preset: &crate::config::SpeakerPreset) -> String {
        let engine_id = preset.engine_id.trim();
        if engine_id.is_empty() {
            preset.engine.as_engine_id().to_string()
        } else {
            engine_id.to_string()
        }
    }

    fn active_engine_id(config: &AppConfig) -> String {
        let engine_id = config.active_engine_id.trim();
        if engine_id.is_empty() {
            config.active_engine.as_engine_id().to_string()
        } else {
            engine_id.to_string()
        }
    }

    fn resolve_engine_for_command(
        registry: &EngineRegistry,
        engine_id: &str,
        config: Option<&EngineCommandConfig>,
    ) -> Result<Arc<dyn TtsEngine>, String> {
        use crate::tts::generic::GenericEngine;

        let normalized_engine_id = Self::normalize_engine_id(engine_id);
        if !registry.contains(&normalized_engine_id) {
            return Err(format!("未対応エンジン: {}", engine_id));
        }
        if normalized_engine_id == ENGINE_ID_GENERIC {
            if let Some(EngineCommandConfig::Generic { url }) = config {
                return Ok(Arc::new(GenericEngine::new(url)));
            }
        }
        registry
            .create(&normalized_engine_id)
            .ok_or_else(|| format!("エンジン生成失敗: {}", engine_id))
    }

    async fn tts_loop(
        voicevox_url: String,
        voiceger_url: String,
        voiceger_ref_audio: String,
        voiceger_prompt_text: String,
        voiceger_prompt_lang: String,
        rx: mpsc::Receiver<TtsCommand>,
        tx: mpsc::Sender<UiMessage>,
    ) {
        use crate::tts::voiceger::VoicegerEngine;

        let dict_engine = VoicevoxEngine::new(&voicevox_url);
        let mut registry = EngineRegistry::new();
        let _ = registry.register_instance(VoicevoxEngine::new(&voicevox_url));

        let voiceger_engine = VoicegerEngine::new(
            &voiceger_url,
            &voiceger_ref_audio,
            &voiceger_prompt_text,
            &voiceger_prompt_lang,
        );
        // Eagerly load Zundamon weights so they're ready before the first synthesis.
        voiceger_engine.load_zundamon_weights().await;
        let _ = registry.register_instance(voiceger_engine);
        let _ = registry.register_factory(|| {
            crate::tts::generic::GenericEngine::new(crate::tts::registry::DEFAULT_GENERIC_URL)
        });

        while let Ok(cmd) = rx.recv() {
            match cmd {
                TtsCommand::Synthesize {
                    text,
                    params,
                    engine_id,
                    config,
                } => {
                    let result = match Self::resolve_engine_for_command(
                        &registry,
                        &engine_id,
                        config.as_ref(),
                    ) {
                        Ok(engine) => engine.synthesize(&text, &params).await,
                        Err(e) => {
                            let _ = tx.send(UiMessage::Error(format!("合成エラー: {}", e)));
                            continue;
                        }
                    };
                    match result {
                        Ok(synth_result) => match synth_result.into_wav_bytes() {
                            Ok(wav) => {
                                let _ = tx.send(UiMessage::WavReady { wav, text });
                            }
                            Err(e) => {
                                let _ = tx.send(UiMessage::Error(format!("音声形式エラー: {}", e)));
                            }
                        },
                        Err(e) => {
                            let _ = tx.send(UiMessage::Error(format!("合成エラー: {}", e)));
                        }
                    }
                }
                TtsCommand::LoadSpeakers { engine_id, config } => {
                    let normalized_engine_id = Self::normalize_engine_id(&engine_id);
                    let engine = match Self::resolve_engine_for_command(
                        &registry,
                        &engine_id,
                        config.as_ref(),
                    ) {
                        Ok(engine) => engine,
                        Err(e) => {
                            let _ = tx.send(UiMessage::Error(format!("スピーカー取得失敗: {}", e)));
                            continue;
                        }
                    };
                    match engine.list_speakers().await {
                        Ok(speakers) => {
                            let _ = tx.send(UiMessage::SpeakersLoaded {
                                engine_id: normalized_engine_id,
                                speakers,
                            });
                        }
                        Err(e) => {
                            if normalized_engine_id != ENGINE_ID_GENERIC {
                                let _ =
                                    tx.send(UiMessage::Error(format!("スピーカー取得失敗: {}", e)));
                            }
                        }
                    }
                }
                TtsCommand::HealthCheck { engine_id, config } => {
                    let normalized_engine_id = Self::normalize_engine_id(&engine_id);
                    let ok = match Self::resolve_engine_for_command(
                        &registry,
                        &engine_id,
                        config.as_ref(),
                    ) {
                        Ok(engine) => engine.health_check().await.unwrap_or(false),
                        Err(_) => false,
                    };
                    let _ = tx.send(UiMessage::HealthCheckResult {
                        engine_id: normalized_engine_id,
                        ok,
                    });
                }
                TtsCommand::LoadUserDict => match dict_engine.list_user_dict().await {
                    Ok(dict) => {
                        let _ = tx.send(UiMessage::UserDictLoaded(dict));
                    }
                    Err(e) => {
                        let _ = tx.send(UiMessage::Error(format!("辞書取得失敗: {}", e)));
                    }
                },
                TtsCommand::AddUserDictWord {
                    surface,
                    pronunciation,
                } => {
                    match dict_engine
                        .add_user_dict_word(&surface, &pronunciation)
                        .await
                    {
                        Ok(_) => {
                            let _ = tx.send(UiMessage::UserDictUpdated);
                        }
                        Err(e) => {
                            let _ = tx.send(UiMessage::Error(format!("辞書登録失敗: {}", e)));
                        }
                    }
                }
                TtsCommand::DeleteUserDictWord { uuid } => {
                    match dict_engine.delete_user_dict_word(&uuid).await {
                        Ok(()) => {
                            let _ = tx.send(UiMessage::UserDictUpdated);
                        }
                        Err(e) => {
                            let _ = tx.send(UiMessage::Error(format!("辞書削除失敗: {}", e)));
                        }
                    }
                }
            }
        }
    }

    fn process_messages(&mut self) {
        while let Ok(msg) = self.ui_rx.try_recv() {
            match msg {
                UiMessage::WavReady { wav, text } => {
                    self.state.is_synthesizing = false;
                    self.state.last_error = None;

                    // Send OSC chatbox message before playback
                    if self.state.config.osc_enabled {
                        tracing::info!(
                            "OSC send: {:?} → {}:{}",
                            text,
                            self.state.config.osc_address,
                            self.state.config.osc_port
                        );
                        if let Err(e) = crate::osc::send_chatbox(
                            &self.state.config.osc_address,
                            self.state.config.osc_port,
                            &text,
                        ) {
                            tracing::warn!("OSC send failed: {}", e);
                        }
                    } else {
                        tracing::info!("OSC disabled, skipping chatbox for: {:?}", text);
                    }

                    let wav = if self.state.config.echo_enabled {
                        crate::audio::effects::apply_echo(
                            &wav,
                            self.state.config.echo_delay_ms,
                            self.state.config.echo_decay,
                        )
                    } else {
                        wav
                    };

                    self.audio_queue.push_back(wav);
                }
                UiMessage::SpeakersLoaded {
                    engine_id,
                    speakers,
                } => {
                    let normalized_engine_id = Self::normalize_engine_id(&engine_id);
                    match normalized_engine_id.as_str() {
                        ENGINE_ID_VOICEVOX => {
                            self.state.speakers = speakers;
                        }
                        ENGINE_ID_GENERIC => {
                            self.state.generic_speakers = speakers;
                        }
                        _ => {}
                    }
                }
                UiMessage::HealthCheckResult { engine_id, ok } => {
                    let normalized_engine_id = Self::normalize_engine_id(&engine_id);
                    match normalized_engine_id.as_str() {
                        ENGINE_ID_VOICEVOX => {
                            let was_disconnected = !self.state.voicevox_connected;
                            self.state.voicevox_connected = ok;
                            if ok {
                                self.state.voicevox_launching = false;
                                self.state.voicevox_auto_launch_pending = false;
                                if was_disconnected {
                                    let _ = self.tts_tx.send(TtsCommand::LoadSpeakers {
                                        engine_id: ENGINE_ID_VOICEVOX.to_string(),
                                        config: None,
                                    });
                                }
                            } else if self.state.voicevox_auto_launch_pending {
                                self.state.voicevox_auto_launch_pending = false;
                                if !self.state.config.voicevox_path.trim().is_empty() {
                                    self.launch_voicevox(false);
                                } else if Self::has_stopped_docker_container() {
                                    // Path is empty but a previously-created container exists — restart it
                                    tracing::info!(
                                        "Auto-starting stopped VOICEVOX Docker container (voicevox_path is empty)"
                                    );
                                    match Command::new("docker")
                                        .args(["start", DOCKER_CONTAINER_NAME])
                                        .stdout(std::process::Stdio::piped())
                                        .stderr(std::process::Stdio::piped())
                                        .spawn()
                                    {
                                        Ok(child) => {
                                            self.voicevox_process = Some(child);
                                            self.is_docker = true;
                                            self.state.voicevox_launching = true;
                                        }
                                        Err(e) => {
                                            tracing::error!(
                                                "Failed to auto-start VOICEVOX container: {}",
                                                e
                                            );
                                        }
                                    }
                                } else {
                                    tracing::info!(
                                        "Skipping VOICEVOX auto-launch: voicevox_path is empty and no Docker container found"
                                    );
                                }
                            }
                        }
                        ENGINE_ID_VOICEGER => {
                            self.state.voiceger_connected = ok;
                            if ok {
                                self.state.voiceger_launching = false;
                                // Server already running — cancel any pending auto-launch
                                // so we don't spawn a second instance (→ address already in use).
                                self.state.voiceger_auto_launch_pending = false;
                            } else if self.state.voiceger_auto_launch_pending {
                                // First health check came back negative: server not running.
                                // Now it's safe to start it.
                                self.state.voiceger_auto_launch_pending = false;
                                self.state.pending_launch_voiceger = true;
                            }
                        }
                        ENGINE_ID_GENERIC => {
                            let was_disconnected = !self.state.generic_connected;
                            self.state.generic_connected = ok;
                            if ok && was_disconnected {
                                let url = self
                                    .state
                                    .config
                                    .active_generic_config()
                                    .map(|e| e.url.clone())
                                    .unwrap_or_default();
                                let _ = self.tts_tx.send(TtsCommand::LoadSpeakers {
                                    engine_id: ENGINE_ID_GENERIC.to_string(),
                                    config: Some(EngineCommandConfig::Generic { url }),
                                });
                            }
                        }
                        _ => {}
                    }
                }
                UiMessage::Error(err) => {
                    self.state.is_synthesizing = false;
                    self.state.last_error = Some(err);
                    self.state.error_display_time = Some(std::time::Instant::now());
                }
                UiMessage::UserDictLoaded(dict) => {
                    self.state.user_dict = dict
                        .into_iter()
                        .map(|(uuid, word)| (uuid, word.surface, word.pronunciation))
                        .collect();
                    self.state.user_dict.sort_by(|a, b| a.1.cmp(&b.1));
                }
                UiMessage::UserDictUpdated => {
                    let _ = self.tts_tx.send(TtsCommand::LoadUserDict);
                }
                UiMessage::LoudnessScanComplete(loudness_map) => {
                    self.state.soundboard_loudness = loudness_map;
                }
            }
        }
    }

    fn periodic_health_check(&mut self) {
        let interval = if self.state.voicevox_launching || self.state.voiceger_launching {
            HEALTH_CHECK_INTERVAL_LAUNCHING_SECS
        } else {
            HEALTH_CHECK_INTERVAL_SECS
        };
        if self.last_health_check.elapsed().as_secs() >= interval {
            self.last_health_check = Instant::now();
            self.check_voiceger_process_exit();
            let _ = self.tts_tx.send(TtsCommand::HealthCheck {
                engine_id: ENGINE_ID_VOICEVOX.to_string(),
                config: None,
            });
            let _ = self.tts_tx.send(TtsCommand::HealthCheck {
                engine_id: ENGINE_ID_VOICEGER.to_string(),
                config: None,
            });
            let generic_url = self
                .state
                .config
                .active_generic_config()
                .map(|e| e.url.clone())
                .unwrap_or_default();
            let _ = self.tts_tx.send(TtsCommand::HealthCheck {
                engine_id: ENGINE_ID_GENERIC.to_string(),
                config: Some(EngineCommandConfig::Generic { url: generic_url }),
            });
        }
    }

    /// If the Voiceger process we spawned has already exited, stop showing
    /// "起動中..." forever and point the user at the server log instead.
    fn check_voiceger_process_exit(&mut self) {
        let Some(proc) = self.voiceger_process.as_mut() else {
            return;
        };
        match proc.try_wait() {
            Ok(Some(status)) => {
                self.voiceger_process = None;
                if !self.state.voiceger_connected {
                    tracing::error!("Voiceger process exited during startup: {}", status);
                    self.state.voiceger_launching = false;
                    self.state.last_error = Some(format!(
                        "Voicegerが起動直後に終了しました ({})。ログ: {}",
                        status, VOICEGER_LOG_PATH
                    ));
                }
            }
            Ok(None) => {}
            Err(e) => tracing::warn!("Failed to poll Voiceger process: {}", e),
        }
    }

    fn process_actions(&mut self) {
        // Handle stop speaking
        if self.state.pending_stop_speaking {
            self.state.pending_stop_speaking = false;
            self.tts_cancel.store(true, Ordering::SeqCst);
            // Clear is_playing immediately so new synthesis isn't blocked
            self.is_playing.store(false, Ordering::SeqCst);
            // Mute the virtual sink immediately — kills audio even if paplay buffers remain
            let sink = &self.state.config.virtual_device_name;
            let _ = std::process::Command::new("pactl")
                .args(["set-sink-mute", sink, "1"])
                .output();
            tracing::info!("TTS stop: muted sink {}", sink);
        }

        // Handle text send
        if let Some(text) = self.state.pending_send.take() {
            // Strip silent words before synthesis
            let text = Self::strip_silent_words(&text, &self.state.config.silent_words);
            if text.is_empty() {
                // Nothing left to speak after stripping
            } else {
                let active_preset = self
                    .state
                    .active_preset_idx
                    .and_then(|i| self.state.config.presets.get(i));
                let engine_id = active_preset
                    .map(Self::preset_engine_id)
                    .unwrap_or_else(|| Self::active_engine_id(&self.state.config));
                let normalized_engine_id = Self::normalize_engine_id(&engine_id);
                let is_voiceger = normalized_engine_id == ENGINE_ID_VOICEGER;
                let is_generic = normalized_engine_id == ENGINE_ID_GENERIC;

                // Voiceger requires a preset — language cannot be inferred without one.
                let voiceger_no_preset = is_voiceger
                    && active_preset
                        .map(|p| {
                            Self::normalize_engine_id(&Self::preset_engine_id(p))
                                != ENGINE_ID_VOICEGER
                        })
                        .unwrap_or(true);
                if voiceger_no_preset {
                    self.state.last_error =
                        Some("Voicegerにはプリセットの選択が必要です。".to_string());
                    // Skip synthesis — fall through to remaining actions
                } else {
                    let params = active_preset
                        .map(|p| {
                            // Voiceger ref override priority:
                            // 1) per-preset custom WAV
                            // 2) per-preset emotion WAV
                            // 3) global voiceger_ref_audio (engine default)
                            let aux_ref_audio = if p.engine
                                == crate::config::TtsEngineType::Voiceger
                            {
                                let preset_ref = p.voiceger_ref_audio_override.trim();
                                if !preset_ref.is_empty() {
                                    Some(preset_ref.to_string())
                                } else if !p.voiceger_emotion.is_empty() {
                                    crate::tts::voiceger::VOICEGER_EMOTIONS
                                        .iter()
                                        .find(|(name, _)| *name == p.voiceger_emotion)
                                        .and_then(|(_, filename)| {
                                            // Derive reference directory from global voiceger_ref_audio.
                                            let base = std::path::Path::new(
                                                &self.state.config.voiceger_ref_audio,
                                            )
                                            .parent()?;
                                            Some(base.join(filename).to_string_lossy().into_owned())
                                        })
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            SynthParams {
                                speaker_id: p.speaker_id,
                                speed_scale: p.synth_params.speed_scale,
                                pitch_scale: p.synth_params.pitch_scale,
                                intonation_scale: p.synth_params.intonation_scale,
                                volume_scale: p.synth_params.volume_scale,
                                aux_ref_audio,
                                voiceger_ref_free: self.state.config.voiceger_ref_free,
                            }
                        })
                        .unwrap_or_else(|| {
                            if is_generic {
                                SynthParams::from_config_generic(&self.state.config)
                            } else {
                                SynthParams::from_config(&self.state.config)
                            }
                        });
                    // Apply Voiceger client-side pronunciation dictionary (per language)
                    let text = if is_voiceger {
                        let lang = crate::tts::voiceger::VoicegerEngine::lang_for_speaker_id(
                            params.speaker_id,
                        );
                        // Warn if detected language doesn't match the preset's expected language
                        if let Some(detected) = Self::detect_text_lang(&text) {
                            if !Self::lang_compatible(lang, detected) {
                                self.state.last_error = Some(format!(
                                "言語の不一致: プリセットは「{}」ですが、テキストは「{}」のようです。",
                                Self::lang_display(lang),
                                Self::lang_display(detected),
                            ));
                            }
                        }
                        Self::apply_voiceger_dict(&text, &self.state.config.voiceger_dict, lang)
                    } else {
                        text
                    };
                    self.state.is_synthesizing = true;
                    // Don't clear last_error here — a lang-mismatch warning may have just been set.
                    let config = if is_generic {
                        Some(EngineCommandConfig::Generic {
                            url: self
                                .state
                                .config
                                .active_generic_config()
                                .map(|e| e.url.clone())
                                .unwrap_or_default(),
                        })
                    } else {
                        None
                    };
                    let _ = self.tts_tx.send(TtsCommand::Synthesize {
                        text,
                        params,
                        engine_id,
                        config,
                    });
                } // end else (voiceger_no_preset)
            }
        }

        // Handle health check
        if self.state.pending_health_check {
            self.state.pending_health_check = false;
            let _ = self.tts_tx.send(TtsCommand::HealthCheck {
                engine_id: ENGINE_ID_VOICEVOX.to_string(),
                config: None,
            });
        }

        // Handle generic engine health check
        if self.state.pending_health_check_generic {
            self.state.pending_health_check_generic = false;
            let url = self
                .state
                .config
                .active_generic_config()
                .map(|e| e.url.clone())
                .unwrap_or_default();
            let _ = self.tts_tx.send(TtsCommand::HealthCheck {
                engine_id: ENGINE_ID_GENERIC.to_string(),
                config: Some(EngineCommandConfig::Generic { url }),
            });
        }

        // Handle engine switch
        if let Some(new_engine) = self.state.pending_switch_engine.take() {
            self.state.config.active_engine = new_engine.clone();
            self.state.config.active_engine_id = new_engine.as_engine_id().to_string();
            let _ = self.state.config.save();

            // Trigger health check + speaker reload for the new engine
            match &new_engine {
                crate::config::TtsEngineType::Voicevox => {
                    self.state.pending_health_check = true;
                    let _ = self.tts_tx.send(TtsCommand::LoadSpeakers {
                        engine_id: ENGINE_ID_VOICEVOX.to_string(),
                        config: None,
                    });
                }
                crate::config::TtsEngineType::Voiceger => {
                    let _ = self.tts_tx.send(TtsCommand::HealthCheck {
                        engine_id: ENGINE_ID_VOICEGER.to_string(),
                        config: None,
                    });
                }
                crate::config::TtsEngineType::Generic => {
                    self.state.pending_health_check_generic = true;
                    let url = self
                        .state
                        .config
                        .active_generic_config()
                        .map(|e| e.url.clone())
                        .unwrap_or_default();
                    let _ = self.tts_tx.send(TtsCommand::LoadSpeakers {
                        engine_id: ENGINE_ID_GENERIC.to_string(),
                        config: Some(EngineCommandConfig::Generic { url }),
                    });
                }
            }

            // Auto-select first preset matching the new engine
            let new_preset_idx = self.state.config.presets.iter().position(|p| {
                if p.engine != new_engine {
                    return false;
                }
                // For Generic, also match the active engine name
                if new_engine == crate::config::TtsEngineType::Generic {
                    return p.generic_engine_name.is_empty()
                        || p.generic_engine_name == self.state.config.active_generic_name();
                }
                true
            });
            self.state.active_preset_idx = new_preset_idx;

            // Apply the selected preset's params to config
            if let Some(idx) = new_preset_idx {
                let preset = &self.state.config.presets[idx];
                match &new_engine {
                    crate::config::TtsEngineType::Voicevox => {
                        self.state.config.speaker_id = preset.speaker_id;
                        self.state.config.synth_params = preset.synth_params.clone();
                    }
                    crate::config::TtsEngineType::Generic => {
                        self.state.config.generic_speaker_id = preset.speaker_id;
                        self.state.config.generic_synth_params = preset.synth_params.clone();
                    }
                    crate::config::TtsEngineType::Voiceger => {}
                }
                let _ = self.state.config.save();
            }
        }

        // Handle VOICEVOX launch
        if self.state.pending_launch_voicevox {
            self.state.pending_launch_voicevox = false;
            self.state.voicevox_auto_launch_pending = false;
            self.launch_voicevox(true);
        }

        // Handle Voiceger launch
        if self.state.pending_launch_voiceger {
            self.state.pending_launch_voiceger = false;
            self.launch_voiceger();
        }

        // Handle VOICEVOX restart — branch by mode
        if self.state.pending_restart_voicevox {
            self.state.pending_restart_voicevox = false;
            let path = self.state.config.voicevox_path.trim().to_string();

            if path.is_empty() {
                // URL-only mode: try to restart a stopped Docker container if one exists,
                // otherwise just reconnect via health check.
                if Self::is_voicevox_docker_running() {
                    tracing::info!("VOICEVOX Docker container already running, reconnecting");
                    self.state.voicevox_connected = false;
                    let _ = self.tts_tx.send(TtsCommand::HealthCheck {
                        engine_id: ENGINE_ID_VOICEVOX.to_string(),
                        config: None,
                    });
                } else if Self::has_stopped_docker_container() {
                    tracing::info!(
                        "Restarting stopped VOICEVOX container (voicevox_path is empty)"
                    );
                    self.state.voicevox_connected = false;
                    match Command::new("docker")
                        .args(["start", DOCKER_CONTAINER_NAME])
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::piped())
                        .spawn()
                    {
                        Ok(child) => {
                            self.voicevox_process = Some(child);
                            self.is_docker = true;
                            self.state.voicevox_launching = true;
                            self.state.last_error = None;
                        }
                        Err(e) => {
                            self.state.last_error =
                                Some(format!("Dockerコンテナ再起動失敗: {}", e));
                        }
                    }
                } else {
                    // No container at all — just try to reconnect
                    self.state.voicevox_connected = false;
                    let _ = self.tts_tx.send(TtsCommand::HealthCheck {
                        engine_id: ENGINE_ID_VOICEVOX.to_string(),
                        config: None,
                    });
                    tracing::info!("VOICEVOX reconnect (URL-only mode, no Docker container)");
                }
            } else {
                // Docker mode: stop the container properly before relaunching
                if self.is_docker && Self::is_voicevox_docker_running() {
                    tracing::info!("Stopping VOICEVOX Docker container for restart");
                    let _ = Command::new("docker")
                        .args(["stop", "-t", "3", DOCKER_CONTAINER_NAME])
                        .status();
                }
                if let Some(mut proc) = self.voicevox_process.take() {
                    let _ = proc.kill();
                }
                self.state.voicevox_connected = false;
                self.state.voicevox_launching = false;
                self.state.voicevox_auto_launch_pending = false;
                self.launch_voicevox(true);
            }
        }

        // Handle Voiceger restart
        if self.state.pending_restart_voiceger {
            self.state.pending_restart_voiceger = false;
            if let Some(mut proc) = self.voiceger_process.take() {
                let _ = proc.kill();
            }
            self.state.voiceger_connected = false;
            self.state.voiceger_launching = false;
            self.launch_voiceger();
        }

        // Handle device creation
        if self.state.pending_create_device {
            self.state.pending_create_device = false;
            self.audio_manager = AudioManager::new(&self.state.config.virtual_device_name);
            // The old device restored the default input on drop; re-apply it if wanted.
            self.default_input_applied = false;
            match self.audio_manager.ensure_device() {
                Ok(()) => {
                    self.state.device_ready = true;
                    self.state.last_error = None;
                }
                Err(e) => {
                    self.state.last_error = Some(format!("デバイス作成失敗: {}", e));
                }
            }
        }

        // Handle device destruction
        if self.state.pending_destroy_device {
            self.state.pending_destroy_device = false;
            match self.audio_manager.destroy_device() {
                Ok(()) => {
                    self.state.device_ready = false;
                    self.default_input_applied = false;
                    self.state.last_error = None;
                }
                Err(e) => {
                    self.state.last_error = Some(format!("デバイス削除失敗: {}", e));
                }
            }
        }

        if self.state.pending_load_user_dict {
            self.state.pending_load_user_dict = false;
            let _ = self.tts_tx.send(TtsCommand::LoadUserDict);
        }
        if let Some((surface, pronunciation)) = self.state.pending_add_dict_word.take() {
            let _ = self.tts_tx.send(TtsCommand::AddUserDictWord {
                surface,
                pronunciation,
            });
        }
        if let Some(uuid) = self.state.pending_delete_dict_word.take() {
            let _ = self.tts_tx.send(TtsCommand::DeleteUserDictWord { uuid });
        }

        if self.state.pending_soundboard_scan {
            self.state.pending_soundboard_scan = false;
            let path = std::path::Path::new(&self.state.config.soundboard_path);
            let mut files = Vec::new();
            if path.is_dir() {
                if let Ok(entries) = std::fs::read_dir(path) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                            if matches!(ext.to_lowercase().as_str(), "wav" | "mp3" | "ogg" | "flac")
                            {
                                let name = p
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("?")
                                    .to_string();
                                files.push((name, p));
                            }
                        }
                    }
                }
            } else {
                let _ = std::fs::create_dir_all(path);
            }
            files.sort_by(|a, b| a.0.cmp(&b.0));
            self.state.soundboard_files = files;
            self.state.pending_loudness_scan = true;
        }

        if self.state.pending_loudness_scan {
            self.state.pending_loudness_scan = false;
            let files: Vec<std::path::PathBuf> = self
                .state
                .soundboard_files
                .iter()
                .map(|(_, path)| path.clone())
                .collect();
            let ui_tx = self.ui_tx.clone();
            std::thread::spawn(move || {
                let mut results = std::collections::HashMap::new();
                for path in &files {
                    match crate::audio::loudness::analyze_loudness(path) {
                        Ok(info) => {
                            results.insert(path.clone(), info);
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Loudness analysis failed for {}: {}",
                                path.display(),
                                e
                            );
                        }
                    }
                }
                let _ = ui_tx.send(UiMessage::LoudnessScanComplete(results));
            });
        }

        if self.state.pending_normalize_all {
            self.state.pending_normalize_all = false;
            for (path, info) in &self.state.soundboard_loudness {
                let gain = crate::audio::loudness::calculate_gain_db(
                    info.lufs,
                    self.state.config.target_lufs,
                );
                let key = path.to_string_lossy().to_string();
                self.state.config.soundboard_gains.insert(key, gain);
            }
            let _ = self.state.config.save();
        }

        if self.state.pending_refresh_mic_sources {
            self.state.pending_refresh_mic_sources = false;
            match crate::audio::virtual_device::VirtualDevice::list_sources() {
                Ok(mut sources) => {
                    // Our own virtual mic must never be the passthrough source (feedback loop).
                    let device = &self.audio_manager.virtual_device;
                    sources.retain(|(name, _)| !device.is_own_source(name));
                    // Auto-select first source if none configured
                    if self.state.config.mic_source_name.is_none() {
                        if let Some((name, _)) = sources.first() {
                            self.state.config.mic_source_name = Some(name.clone());
                        }
                    }
                    self.state.available_mic_sources = sources;
                }
                Err(e) => {
                    tracing::warn!("Failed to list mic sources: {}", e);
                }
            }
        }

        if self.state.pending_toggle_mic {
            self.state.pending_toggle_mic = false;
            let was_on = self.audio_manager.virtual_device.is_mic_passthrough();
            tracing::info!(
                "Mic toggle requested: currently {}",
                if was_on { "ON" } else { "OFF" }
            );
            let result = if was_on {
                self.audio_manager.virtual_device.disable_mic_passthrough()
            } else {
                let source = self.mic_source();
                tracing::info!("Enabling mic passthrough with source: {}", source);
                // Ensure sink is unmuted (STOP may have muted it)
                let _ = std::process::Command::new("pactl")
                    .args(["set-sink-mute", &self.state.config.virtual_device_name, "0"])
                    .output();
                self.enable_mic_passthrough(&source)
            };
            match result {
                Ok(()) => {
                    self.state.mic_passthrough =
                        self.audio_manager.virtual_device.is_mic_passthrough();
                    tracing::info!("Mic passthrough is now: {}", self.state.mic_passthrough);
                    // Restart capture with updated speaker routing to prevent acoustic feedback
                    if self.state.is_capturing {
                        let virtual_sink = self.state.config.virtual_device_name.clone();
                        if let Err(e) = self
                            .desktop_capture
                            .restart_capture(&virtual_sink, self.state.mic_passthrough)
                        {
                            tracing::warn!("Failed to restart capture after mic toggle: {}", e);
                        }
                    }
                }
                Err(e) => {
                    self.state.last_error = Some(format!("マイク切替失敗: {}", e));
                }
            }
        }

        // Reconnect mic loopback with newly selected source
        if self.state.pending_reconnect_mic {
            self.state.pending_reconnect_mic = false;
            let source = self.mic_source();
            tracing::info!("Reconnecting mic loopback with source: {}", source);
            let _ = self.audio_manager.virtual_device.disable_mic_passthrough();
            match self.enable_mic_passthrough(&source) {
                Ok(()) => {
                    self.state.mic_passthrough =
                        self.audio_manager.virtual_device.is_mic_passthrough();
                }
                Err(e) => {
                    self.state.last_error = Some(format!("マイク再接続失敗: {}", e));
                    self.state.mic_passthrough = false;
                }
            }
        }

        if self.state.pending_soundboard_stop {
            self.state.pending_soundboard_stop = false;
            crate::audio::playback::stop_file_playback(
                &self.soundboard_pids,
                &self.soundboard_cancel,
            );
        }

        if let Some(file_path) = self.state.pending_soundboard_play.take() {
            if self.is_soundboard_playing.load(Ordering::SeqCst) {
                // Stop current playback and start new one
                crate::audio::playback::stop_file_playback(
                    &self.soundboard_pids,
                    &self.soundboard_cancel,
                );
            }
            // Reset cancel signal for new playback
            self.soundboard_cancel.store(false, Ordering::SeqCst);
            let gain_db = self
                .state
                .config
                .soundboard_gains
                .get(&file_path.to_string_lossy().to_string())
                .copied();
            let device_name = self.state.config.virtual_device_name.clone();
            let monitor = self.state.config.monitor_audio;
            let playing = self.is_soundboard_playing.clone();
            let pids = self.soundboard_pids.clone();
            let cancel = self.soundboard_cancel.clone();
            playing.store(true, Ordering::SeqCst);
            std::thread::spawn(move || {
                let _guard = PlaybackGuard(playing);
                if let Err(e) = crate::audio::playback::play_file(
                    &file_path,
                    &device_name,
                    monitor,
                    pids,
                    cancel,
                    gain_db,
                ) {
                    tracing::error!("Soundboard playback error: {}", e);
                }
            });
        }

        if self.state.pending_refresh_sink_inputs {
            self.state.pending_refresh_sink_inputs = false;
            match crate::media::desktop_capture::DesktopCapture::list_sink_inputs() {
                Ok(inputs) => self.state.sink_inputs = inputs,
                Err(e) => self.state.last_error = Some(format!("sink-input取得失敗: {}", e)),
            }
        }

        if let Some((input_id, original_sink)) = self.state.pending_start_capture.take() {
            let virtual_sink = &self.state.config.virtual_device_name;
            let skip_speaker = self.state.mic_passthrough;
            match self.desktop_capture.start_capture(
                input_id,
                &original_sink,
                virtual_sink,
                skip_speaker,
            ) {
                Ok(()) => {
                    self.state.is_capturing = true;
                    self.state.last_error = None;
                }
                Err(e) => {
                    self.state.last_error = Some(format!("キャプチャ開始失敗: {}", e));
                }
            }
        }

        if self.state.pending_stop_capture {
            self.state.pending_stop_capture = false;
            self.desktop_capture.stop_capture();
            self.state.is_capturing = false;
        }
    }

    /// Remove words marked as silent from text before synthesis.
    fn strip_silent_words(text: &str, silent_words: &[String]) -> String {
        let mut result = text.to_string();
        for word in silent_words {
            result = result.replace(word.as_str(), "");
        }
        result.trim().to_string()
    }

    /// Heuristic language detector for Voiceger language mismatch warnings.
    /// Returns the detected lang code, or None if the text is too short/ambiguous.
    fn detect_text_lang(text: &str) -> Option<&'static str> {
        let mut hiragana_katakana = 0u32;
        let mut hangul = 0u32;
        let mut cjk = 0u32;
        let mut latin = 0u32;
        let mut total = 0u32;

        for c in text.chars() {
            if c.is_whitespace() || c.is_ascii_punctuation() {
                continue;
            }
            total += 1;
            let cp = c as u32;
            if (0x3040..=0x30FF).contains(&cp) {
                hiragana_katakana += 1;
            } else if (0xAC00..=0xD7AF).contains(&cp) || (0x1100..=0x11FF).contains(&cp) {
                hangul += 1;
            } else if (0x4E00..=0x9FFF).contains(&cp) || (0x3400..=0x4DBF).contains(&cp) {
                cjk += 1;
            } else if c.is_ascii_alphabetic() {
                latin += 1;
            }
        }

        if total < 2 {
            return None; // too short to judge
        }

        // Hiragana/katakana is a definitive marker for Japanese
        if hiragana_katakana > 0 {
            return Some("ja");
        }
        if hangul > 0 {
            return Some("ko");
        }
        // CJK without kana = Chinese or Cantonese (we can't easily distinguish)
        if cjk * 2 > total {
            return Some("zh");
        }
        // Majority Latin = English
        if latin * 2 > total {
            return Some("en");
        }
        None
    }

    /// Returns true if the detected language is compatible with the preset language.
    /// zh and yue share CJK script so they're treated as compatible.
    fn lang_compatible(preset_lang: &str, detected: &str) -> bool {
        if preset_lang == detected {
            return true;
        }
        // zh and yue are visually indistinguishable by simple char analysis
        matches!(
            (preset_lang, detected),
            ("zh", "yue") | ("yue", "zh") | ("zh", "zh") | ("yue", "yue")
        )
    }

    fn lang_display(lang: &str) -> &'static str {
        match lang {
            "ja" => "日本語",
            "en" => "English",
            "zh" => "中文",
            "ko" => "한국어",
            "yue" => "粤語",
            _ => "不明",
        }
    }

    /// Apply Voiceger client-side pronunciation dictionary for a given language (longest-match).
    fn apply_voiceger_dict(
        text: &str,
        dict: &std::collections::HashMap<String, std::collections::HashMap<String, String>>,
        lang: &str,
    ) -> String {
        let lang_dict = match dict.get(lang) {
            Some(d) if !d.is_empty() => d,
            _ => return text.to_string(),
        };
        let mut entries: Vec<(&str, &str)> = lang_dict
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        entries.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

        let mut result = String::with_capacity(text.len());
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let remaining: String = chars[i..].iter().collect();
            if let Some((surface, reading)) = entries.iter().find(|(s, _)| remaining.starts_with(s))
            {
                result.push_str(reading);
                i += surface.chars().count();
            } else {
                result.push(chars[i]);
                i += 1;
            }
        }
        result
    }

    fn is_voicevox_docker_running() -> bool {
        std::process::Command::new("docker")
            .args([
                "ps",
                "--filter",
                DOCKER_NAME_FILTER,
                "--format",
                "{{.Names}}",
            ])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(DOCKER_CONTAINER_NAME))
            .unwrap_or(false)
    }

    fn is_docker_command(cmd: &str) -> bool {
        let trimmed = cmd.trim_start();
        trimmed.starts_with("docker ") || trimmed.starts_with("docker run")
    }

    /// Check if a stopped container with our name exists and can be restarted.
    fn has_stopped_docker_container() -> bool {
        Command::new("docker")
            .args([
                "ps",
                "-a",
                "--filter",
                DOCKER_NAME_FILTER,
                "--filter",
                "status=exited",
                "--format",
                "{{.Names}}",
            ])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(DOCKER_CONTAINER_NAME))
            .unwrap_or(false)
    }

    fn launch_voicevox(&mut self, user_requested: bool) {
        let path = self.state.config.voicevox_path.trim().to_string();
        if path.is_empty() {
            self.state.voicevox_launching = false;
            if user_requested {
                self.state.last_error = Some(
                    "VOICEVOX実行パスが未設定です。URL接続のみで使う場合は「接続テスト」を使ってください。ローカル起動には実行パスを設定してください。"
                        .to_string(),
                );
            } else {
                tracing::info!("Skipping VOICEVOX launch: voicevox_path is empty");
            }
            return;
        }

        let is_docker = Self::is_docker_command(&path);

        // Duplicate guard — for Docker, check container; for local, check process
        if is_docker && Self::is_voicevox_docker_running() {
            tracing::info!("VOICEVOX Docker container already running");
            self.state.voicevox_launching = true;
            return;
        }

        // Local process guard
        if let Some(ref mut proc) = self.voicevox_process {
            match proc.try_wait() {
                Ok(None) => {
                    tracing::info!("VOICEVOX process already running");
                    return;
                }
                _ => {
                    self.voicevox_process = None;
                }
            }
        }

        // If a stopped container exists, restart it instead of creating a new one
        if is_docker && Self::has_stopped_docker_container() {
            tracing::info!("Restarting stopped VOICEVOX container");
            let result = Command::new("docker")
                .args(["start", DOCKER_CONTAINER_NAME])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn();
            match result {
                Ok(child) => {
                    self.voicevox_process = Some(child);
                    self.is_docker = true;
                    self.state.last_error = None;
                    self.state.voicevox_launching = true;
                    tracing::info!("VOICEVOX container restarted");
                }
                Err(e) => {
                    tracing::error!("Failed to restart VOICEVOX container: {}", e);
                    self.state.last_error = Some(format!("VOICEVOX再起動失敗: {}", e));
                    self.state.voicevox_launching = false;
                }
            }
            return;
        }

        tracing::info!("Launching VOICEVOX: {}", path);

        let result = if is_docker {
            Self::launch_docker_voicevox(&path, &self.state.config.voicevox_url)
        } else {
            Self::launch_local_voicevox(&path)
        };

        match result {
            Ok(child) => {
                self.voicevox_process = Some(child);
                self.is_docker = is_docker;
                self.state.last_error = None;
                self.state.voicevox_launching = true;
                tracing::info!("VOICEVOX process spawned");
            }
            Err(e) => {
                tracing::error!("Failed to launch VOICEVOX: {}", e);
                self.state.last_error = Some(format!("VOICEVOX起動失敗: {}", e));
            }
        }
    }

    fn launch_voiceger(&mut self) {
        let path = self.state.config.effective_voiceger_launch_cmd();

        // Guard: already running
        if let Some(ref mut proc) = self.voiceger_process {
            match proc.try_wait() {
                Ok(None) => {
                    tracing::info!("Voiceger process already running");
                    return;
                }
                _ => {
                    self.voiceger_process = None;
                }
            }
        }

        tracing::info!("Launching Voiceger: {}", path);
        let words = match shell_words::split(&path) {
            Ok(w) if !w.is_empty() => w,
            _ => {
                self.state.last_error =
                    Some("Voiceger起動コマンドのパースに失敗しました".to_string());
                return;
            }
        };

        // Run from the GPT-SoVITS directory so relative imports work
        let workdir = self
            .state
            .config
            .voiceger_base_dir()
            .map(|d| d.join("GPT-SoVITS"))
            .filter(|d| d.exists());

        // Write server output to a log file for debugging.
        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(VOICEGER_LOG_PATH)
            .ok();
        if let Some(mut f) = log_file.as_ref() {
            use std::io::Write as _;
            let _ = writeln!(
                f,
                "=== zundux: launching Voiceger: {} (cwd: {}) ===",
                path,
                workdir
                    .as_ref()
                    .map(|d| d.display().to_string())
                    .unwrap_or_else(|| "<inherited>".to_string())
            );
        }
        let (stdout_io, stderr_io) = match log_file {
            Some(f) => {
                let f2 = f.try_clone().unwrap_or_else(|_| {
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(VOICEGER_LOG_PATH)
                        .unwrap()
                });
                (std::process::Stdio::from(f), std::process::Stdio::from(f2))
            }
            None => (std::process::Stdio::null(), std::process::Stdio::null()),
        };

        let mut cmd = Command::new(&words[0]);
        cmd.args(&words[1..])
            .env("PYTORCH_CUDA_ALLOC_CONF", "expandable_segments:True")
            .stdout(stdout_io)
            .stderr(stderr_io);
        if let Some(dir) = workdir {
            cmd.current_dir(dir);
        }
        let result = cmd.spawn();

        match result {
            Ok(child) => {
                self.voiceger_process = Some(child);
                self.state.last_error = None;
                self.state.voiceger_launching = true;
                tracing::info!("Voiceger process spawned");
            }
            Err(e) => {
                tracing::error!("Failed to launch Voiceger: {}", e);
                self.state.last_error = Some(format!("Voiceger起動失敗: {}", e));
            }
        }
    }

    fn launch_docker_voicevox(path: &str, _url: &str) -> anyhow::Result<std::process::Child> {
        let words = shell_words::split(path)
            .map_err(|e| anyhow::anyhow!("Failed to parse docker command: {}", e))?;

        if words.is_empty() {
            anyhow::bail!("Empty docker command");
        }

        // Reject shell metacharacters in any argument
        for word in &words {
            if word.chars().any(|c| {
                matches!(
                    c,
                    ';' | '|' | '&' | '$' | '`' | '(' | ')' | '{' | '}' | '<' | '>'
                )
            }) {
                anyhow::bail!(
                    "Shell metacharacter detected in docker command argument: {}",
                    word
                );
            }
        }

        // Insert --name and -d flags before the image name.
        // Docker syntax: docker run [OPTIONS] IMAGE [COMMAND] [ARG...]
        // We need to find where "run" is, then insert our flags right after
        // the user-supplied options but before the image name.
        // Strategy: find "run" in args, then find the first positional arg
        // (not starting with '-' and not a value for a preceding flag) — that's the image.
        let mut args: Vec<String> = words[1..].to_vec();

        // Find image position: skip flags and their values after "run"
        let run_idx = args.iter().position(|w| w == "run");
        let search_start = run_idx.map_or(0, |i| i + 1);
        let mut i = search_start;
        while i < args.len() {
            let arg = &args[i];
            if arg.starts_with('-') {
                // Flags that take a value (next arg is consumed)
                let takes_value = matches!(
                    arg.as_str(),
                    "-p" | "--publish"
                        | "-v"
                        | "--volume"
                        | "-e"
                        | "--env"
                        | "--name"
                        | "--gpus"
                        | "--network"
                        | "--platform"
                        | "-w"
                        | "--workdir"
                        | "-u"
                        | "--user"
                        | "--entrypoint"
                        | "--mount"
                        | "-l"
                        | "--label"
                        | "--memory"
                        | "-m"
                );
                if takes_value && !arg.contains('=') {
                    i += 2; // skip flag + value
                } else {
                    i += 1; // boolean flag or --flag=value
                }
            } else {
                // First positional arg = image name
                break;
            }
        }

        // Insert our flags right before the image name
        args.insert(i, "-d".to_string());
        args.insert(i, DOCKER_CONTAINER_NAME.to_string());
        args.insert(i, "--name".to_string());

        let child = std::process::Command::new(&words[0])
            .args(&args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to spawn docker command")?;

        Ok(child)
    }

    fn launch_local_voicevox(path: &str) -> anyhow::Result<std::process::Child> {
        let words = shell_words::split(path)
            .map_err(|e| anyhow::anyhow!("Failed to parse voicevox command: {}", e))?;

        if words.is_empty() {
            anyhow::bail!("Empty voicevox command");
        }

        // Reject shell metacharacters
        for word in &words {
            if word.chars().any(|c| {
                matches!(
                    c,
                    ';' | '|' | '&' | '$' | '`' | '(' | ')' | '{' | '}' | '<' | '>'
                )
            }) {
                anyhow::bail!("Shell metacharacter detected in voicevox command: {}", word);
            }
        }

        // Write server output to a log file for debugging.
        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/tmp/zundux_voicevox.log")
            .ok();
        let (stdout_io, stderr_io) = match log_file {
            Some(f) => {
                let f2 = f.try_clone().unwrap_or_else(|_| {
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open("/tmp/zundux_voicevox.log")
                        .unwrap()
                });
                (std::process::Stdio::from(f), std::process::Stdio::from(f2))
            }
            None => (std::process::Stdio::null(), std::process::Stdio::null()),
        };

        let child = std::process::Command::new(&words[0])
            .args(&words[1..])
            .stdout(stdout_io)
            .stderr(stderr_io)
            .spawn()
            .context("Failed to spawn voicevox process")?;

        Ok(child)
    }
}

impl Drop for ZunduxApp {
    fn drop(&mut self) {
        self.desktop_capture.stop_capture();
        if self.is_docker {
            // Graceful: try stop first (sends SIGTERM to container)
            let _ = std::process::Command::new("docker")
                .args(["stop", "-t", "5", DOCKER_CONTAINER_NAME])
                .status();
        }
        if let Some(ref mut child) = self.voicevox_process {
            // Try graceful SIGTERM first, then force kill after 5 seconds
            let _ = std::process::Command::new("kill")
                .args(["-TERM", &child.id().to_string()])
                .status();
            match child.try_wait() {
                Ok(Some(_)) => {}
                _ => {
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
    }
}

impl eframe::App for ZunduxApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // Transparent clear color for the window
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_messages();
        self.state.is_playing = self.is_playing.load(Ordering::SeqCst);

        // Drain audio queue: start next playback when current finishes.
        if !self.state.is_playing {
            if let Some(wav) = self.audio_queue.pop_front() {
                let _ = std::process::Command::new("pactl")
                    .args(["set-sink-mute", &self.state.config.virtual_device_name, "0"])
                    .output();
                let device_name = self.state.config.virtual_device_name.clone();
                let monitor = self.state.config.monitor_audio;
                let playing = self.is_playing.clone();
                let cancel = self.tts_cancel.clone();
                cancel.store(false, Ordering::SeqCst);
                playing.store(true, Ordering::SeqCst);
                self.state.is_playing = true;
                std::thread::spawn(move || {
                    let _guard = PlaybackGuard(playing);
                    if let Err(e) =
                        crate::audio::playback::play_wav(wav, &device_name, monitor, cancel)
                    {
                        tracing::error!("Playback error: {}", e);
                    }
                });
            }
        }
        self.state.is_soundboard_playing = self.is_soundboard_playing.load(Ordering::SeqCst);
        self.periodic_health_check();
        self.sync_audio_settings();
        self.process_actions();

        let theme = &self.state.config.theme;

        // Apply theme visuals when needed
        if self.state.needs_theme_update {
            self.needs_theme_update = true;
            self.state.needs_theme_update = false;
        }
        if self.needs_theme_update {
            ctx.set_visuals(theme.to_visuals());
            ctx.set_style(theme.to_style());
            self.needs_theme_update = false;
        }

        // Paint window background with rounded rect
        let is_maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        let rounding = if is_maximized {
            0.0
        } else {
            theme.window_rounding
        };
        let screen_rect = ctx.screen_rect();

        // Persist window size when it changes (skip while maximized)
        if !is_maximized {
            let (w, h) = (screen_rect.width(), screen_rect.height());
            let saved_w = self.state.config.window_width;
            let saved_h = self.state.config.window_height;
            if (w - saved_w).abs() > 1.0 || (h - saved_h).abs() > 1.0 {
                self.state.config.window_width = w;
                self.state.config.window_height = h;
                let _ = self.state.config.save();
            }
        }
        let painter = ctx.layer_painter(egui::LayerId::background());
        painter.rect_filled(
            screen_rect,
            egui::CornerRadius::same(rounding as u8),
            theme.color(theme.window_background),
        );

        // Custom title bar
        crate::ui::titlebar::show(ctx, theme);

        // Keyboard shortcuts
        if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::F4)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // Tab panel
        egui::TopBottomPanel::top("tabs")
            .frame(
                egui::Frame::NONE
                    .fill(egui::Color32::TRANSPARENT)
                    .inner_margin(egui::Margin::symmetric(
                        theme.spacing_medium as i8,
                        theme.spacing_small as i8,
                    )),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (screen, label) in [
                        (Screen::Input, "Input"),
                        (Screen::Soundboard, "Soundboard"),
                        (Screen::Settings, "Settings"),
                    ] {
                        let is_active = self.state.current_screen == screen;
                        let bg = if is_active {
                            theme.color(theme.tab_active_background)
                        } else {
                            egui::Color32::TRANSPARENT
                        };
                        let text_color = if is_active {
                            theme.color(theme.text_primary)
                        } else {
                            theme.color(theme.text_muted)
                        };
                        let btn = ui.add(
                            egui::Button::new(
                                egui::RichText::new(label).size(11.0).color(text_color),
                            )
                            .fill(bg)
                            .corner_radius(egui::CornerRadius::same(theme.tab_rounding as u8)),
                        );
                        if btn.clicked() {
                            self.state.current_screen = screen;
                            if screen == Screen::Input {
                                self.state.needs_initial_focus = true;
                            }
                        }
                    }
                });
            });

        // Status bar (bottom)
        let theme = &self.state.config.theme;
        egui::TopBottomPanel::bottom("status")
            .exact_height(24.0)
            .frame(egui::Frame::NONE.fill(theme.color(theme.titlebar_background)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    let engine_name = match self.state.config.active_engine {
                        crate::config::TtsEngineType::Voicevox => "VOICEVOX",
                        crate::config::TtsEngineType::Voiceger => "Voiceger",
                        crate::config::TtsEngineType::Generic => {
                            self.state.config.active_generic_name()
                        }
                    };
                    let (connected, launching) = match self.state.config.active_engine {
                        crate::config::TtsEngineType::Voicevox => {
                            (self.state.voicevox_connected, self.state.voicevox_launching)
                        }
                        crate::config::TtsEngineType::Voiceger => {
                            (self.state.voiceger_connected, self.state.voiceger_launching)
                        }
                        crate::config::TtsEngineType::Generic => {
                            (self.state.generic_connected, false)
                        }
                    };
                    let (vox_color, vox_text) = if connected {
                        (theme.color(theme.status_ok), engine_name)
                    } else if launching {
                        (theme.color(theme.status_warn), engine_name)
                    } else {
                        (theme.color(theme.status_error), engine_name)
                    };
                    ui.colored_label(vox_color, format!("\u{25CF} {}", vox_text));
                    ui.add_space(12.0);
                    let (mic_color, mic_text) = if self.state.device_ready {
                        (theme.color(theme.status_ok), "Virtual Mic")
                    } else {
                        (theme.color(theme.status_warn), "Virtual Mic")
                    };
                    ui.colored_label(mic_color, format!("\u{25CF} {}", mic_text));
                    if let Some(ref error) = self.state.last_error.clone() {
                        ui.add_space(12.0);
                        let error_label = ui.colored_label(
                            theme.color(theme.status_error),
                            error.chars().take(60).collect::<String>(),
                        );
                        self.state.error_hovered = error_label.hovered();
                        if error_label.clicked() {
                            self.state.last_error = None;
                            self.state.error_display_time = None;
                        }
                    }

                    // Resize handle at bottom-right corner
                    let available = ui.available_size();
                    let handle_size = available.y;
                    // Label sits outside (left of) the hit area
                    ui.add_space(available.x - handle_size - 60.0);
                    ui.label(
                        egui::RichText::new("サイズ変更")
                            .size(10.0)
                            .color(theme.color(theme.text_muted)),
                    );
                    // Square hit area
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(handle_size, handle_size),
                        egui::Sense::drag(),
                    );
                    ui.painter().rect_filled(
                        rect,
                        egui::CornerRadius::ZERO,
                        theme.color(theme.resize_handle_background),
                    );
                    ui.painter().rect_stroke(
                        rect,
                        egui::CornerRadius::ZERO,
                        egui::Stroke::new(1.0, theme.color(theme.text_muted)),
                        egui::epaint::StrokeKind::Inside,
                    );
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "\u{2921}",
                        egui::FontId::proportional(12.0),
                        theme.color(theme.text_muted),
                    );
                    if response.contains_pointer() && ui.input(|i| i.pointer.primary_pressed()) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(
                            egui::ResizeDirection::SouthEast,
                        ));
                    }
                });
            });

        // Toast auto-dismiss (5 seconds, paused on hover)
        if let Some(time) = self.state.error_display_time {
            if !self.state.error_hovered && time.elapsed() > std::time::Duration::from_secs(5) {
                self.state.last_error = None;
                self.state.error_display_time = None;
            }
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(egui::Color32::TRANSPARENT)
                    .inner_margin(egui::Margin::symmetric(12, 0)),
            )
            .show(ctx, |ui| match self.state.current_screen {
                Screen::Input => crate::ui::input::show(ui, &mut self.state),
                Screen::Soundboard => crate::ui::soundboard::show(ui, &mut self.state),
                Screen::Settings => crate::ui::settings::show(ui, &mut self.state),
            });

        // Keep repainting for periodic health checks and spinner
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_shell_metacharacters_in_docker_cmd() {
        let result =
            ZunduxApp::launch_docker_voicevox("docker run evil;rm -rf /", "http://127.0.0.1:50021");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("metacharacter"));
    }

    #[test]
    fn rejects_shell_metacharacters_in_local_cmd() {
        let result = ZunduxApp::launch_local_voicevox("voicevox && rm -rf /");
        assert!(result.is_err());
    }
}
