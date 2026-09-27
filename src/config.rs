use crate::ui::theme::Theme;
use crate::validation;
use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TtsEngineType {
    #[default]
    Voicevox,
    Voiceger,
    Generic,
}

pub const ENGINE_ID_VOICEVOX: &str = "voicevox";
pub const ENGINE_ID_VOICEGER: &str = "voiceger";
pub const ENGINE_ID_GENERIC: &str = "generic";

impl TtsEngineType {
    pub fn as_engine_id(&self) -> &'static str {
        match self {
            Self::Voicevox => ENGINE_ID_VOICEVOX,
            Self::Voiceger => ENGINE_ID_VOICEGER,
            Self::Generic => ENGINE_ID_GENERIC,
        }
    }

    pub fn from_engine_id(engine_id: &str) -> Option<Self> {
        let engine_id = engine_id.trim();
        if engine_id.eq_ignore_ascii_case(ENGINE_ID_VOICEVOX) {
            Some(Self::Voicevox)
        } else if engine_id.eq_ignore_ascii_case(ENGINE_ID_VOICEGER) {
            Some(Self::Voiceger)
        } else if engine_id.eq_ignore_ascii_case(ENGINE_ID_GENERIC)
            || engine_id
                .to_ascii_lowercase()
                .starts_with(&format!("{ENGINE_ID_GENERIC}:"))
        {
            Some(Self::Generic)
        } else {
            None
        }
    }
}

/// VOICEVOX互換エンジン一つ分の設定。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenericEngineConfig {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub voicevox_url: String,
    pub voicevox_path: String,
    pub auto_launch_voicevox: bool,
    #[serde(default)]
    pub auto_launch_voiceger: bool,
    pub auto_start_app: bool,
    pub synth_params: SynthParamsConfig,
    pub speaker_id: u32,
    pub virtual_device_name: String,
    pub monitor_audio: bool,
    pub templates: Vec<String>,
    pub osc_enabled: bool,
    pub osc_address: String,
    pub osc_port: u16,
    pub soundboard_path: String,
    pub mic_source_name: Option<String>,
    pub echo_enabled: bool,
    pub echo_delay_ms: u32,
    pub echo_decay: f64,
    /// true = 音声エフェクト（エコー等）を合成音声のみに適用し、マイク入力には掛けない。
    /// false (default) = マイク入力にも同じエフェクトを掛ける。
    #[serde(default)]
    pub effects_tts_only: bool,
    /// Zunduxの仮想マイクをシステム（PC）の既定の入力デバイスにする。
    #[serde(default)]
    pub use_as_default_input: bool,
    #[serde(default = "default_target_lufs")]
    pub target_lufs: f64,
    #[serde(default = "default_loudness_tolerance")]
    pub loudness_tolerance: f64,
    #[serde(default)]
    pub soundboard_gains: std::collections::HashMap<String, f64>,
    #[serde(default)]
    pub noise_suppression: bool,
    #[serde(default)]
    pub silent_words: Vec<String>,
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub presets: Vec<SpeakerPreset>,
    #[serde(default)]
    pub templates_default_expanded: bool,
    #[serde(default = "default_window_width")]
    pub window_width: f32,
    #[serde(default = "default_window_height")]
    pub window_height: f32,
    #[serde(default)]
    pub active_engine: TtsEngineType,
    #[serde(default)]
    pub active_engine_id: String,
    #[serde(default)]
    pub engine_configs: std::collections::HashMap<String, serde_json::Value>,
    #[serde(default = "default_voiceger_url")]
    pub voiceger_url: String,
    #[serde(default)]
    pub voiceger_path: String,
    #[serde(default)]
    pub voiceger_ref_audio: String,
    #[serde(default)]
    pub voiceger_prompt_text: String,
    #[serde(default = "default_voiceger_prompt_lang")]
    pub voiceger_prompt_lang: String,
    #[serde(default)]
    pub voiceger_ref_free: bool,
    /// Per-language client-side text replacements applied before Voiceger synthesis.
    /// Outer key = language code (ja/en/zh/ko/yue), inner key = surface, value = reading.
    #[serde(default)]
    pub voiceger_dict: std::collections::HashMap<String, std::collections::HashMap<String, String>>,
    /// VOICEVOX互換エンジンのリスト。
    #[serde(default = "default_generic_engines")]
    pub generic_engines: Vec<GenericEngineConfig>,
    /// アクティブな汎用エンジンのインデックス。
    #[serde(default)]
    pub active_generic_engine_idx: usize,
    /// 汎用エンジンのデフォルトスピーカーID（プリセット未選択時に使用）。
    #[serde(default)]
    pub generic_speaker_id: u32,
    /// 汎用エンジンのグローバル音声パラメータ（プリセット未選択時に使用）。
    #[serde(default)]
    pub generic_synth_params: SynthParamsConfig,
    /// 旧設定 — 移行用（シリアライズしない）
    #[serde(
        default = "default_generic_url",
        rename = "generic_url",
        skip_serializing
    )]
    pub generic_url_legacy: String,
    /// 旧設定 — 移行用（シリアライズしない）
    #[serde(
        default = "default_generic_engine_name",
        rename = "generic_engine_name",
        skip_serializing
    )]
    pub generic_engine_name_legacy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SynthParamsConfig {
    pub speed_scale: f64,
    pub pitch_scale: f64,
    pub intonation_scale: f64,
    pub volume_scale: f64,
    #[serde(default = "default_engine_params")]
    pub engine_params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeakerPreset {
    pub name: String,
    pub speaker_id: u32,
    pub synth_params: SynthParamsConfig,
    #[serde(default)]
    pub engine: TtsEngineType,
    #[serde(default)]
    pub engine_id: String,
    /// Voiceger emotion name (e.g. "甘え"). Empty string = ノーマル (use global ref audio).
    #[serde(default)]
    pub voiceger_emotion: String,
    /// Voiceger only: optional per-preset reference wav path.
    /// If set, this takes precedence over emotion/global reference audio.
    #[serde(default)]
    pub voiceger_ref_audio_override: String,
    /// Generic engine: このプリセットが属するエンジン名。
    /// 空文字列の場合はすべてのGenericエンジンで表示される（後方互換）。
    #[serde(default)]
    pub generic_engine_name: String,
}

impl SpeakerPreset {
    fn inferred_engine_id_from_legacy(&self) -> String {
        match self.engine {
            TtsEngineType::Voicevox => ENGINE_ID_VOICEVOX.to_string(),
            TtsEngineType::Voiceger => ENGINE_ID_VOICEGER.to_string(),
            TtsEngineType::Generic => {
                let name = self.generic_engine_name.trim();
                if name.is_empty() {
                    ENGINE_ID_GENERIC.to_string()
                } else {
                    format!("{ENGINE_ID_GENERIC}:{name}")
                }
            }
        }
    }

    fn sync_engine_compat_fields(&mut self) {
        let current_engine_id = self.engine_id.trim();
        if current_engine_id.is_empty() {
            self.engine_id = self.inferred_engine_id_from_legacy();
            return;
        }

        if let Some(engine) = TtsEngineType::from_engine_id(current_engine_id) {
            self.engine = engine;
            if engine == TtsEngineType::Generic {
                if self.generic_engine_name.trim().is_empty() {
                    if let Some((prefix, name)) = current_engine_id.split_once(':') {
                        if prefix.eq_ignore_ascii_case(ENGINE_ID_GENERIC) && !name.trim().is_empty()
                        {
                            self.generic_engine_name = name.trim().to_string();
                        }
                    }
                }
                if current_engine_id.eq_ignore_ascii_case(ENGINE_ID_GENERIC) {
                    self.engine_id = ENGINE_ID_GENERIC.to_string();
                }
            } else {
                self.engine_id = engine.as_engine_id().to_string();
            }
        }
    }

    fn sync_legacy_preset_engine_params(&mut self) {
        self.synth_params.ensure_engine_params_object();
        let params = self.synth_params.engine_params_object_mut();

        let emotion_key = "voiceger_emotion".to_string();
        if self.voiceger_emotion.trim().is_empty() {
            if let Some(value) = params.get(&emotion_key).and_then(JsonValue::as_str) {
                self.voiceger_emotion = value.to_string();
            }
        } else {
            params.insert(
                emotion_key,
                JsonValue::String(self.voiceger_emotion.clone()),
            );
        }

        let ref_audio_key = "voiceger_ref_audio_override".to_string();
        if self.voiceger_ref_audio_override.trim().is_empty() {
            if let Some(value) = params.get(&ref_audio_key).and_then(JsonValue::as_str) {
                self.voiceger_ref_audio_override = value.to_string();
            }
        } else {
            params.insert(
                ref_audio_key,
                JsonValue::String(self.voiceger_ref_audio_override.clone()),
            );
        }
    }
}

fn default_target_lufs() -> f64 {
    -14.0
}

fn default_loudness_tolerance() -> f64 {
    3.0
}

fn default_window_width() -> f32 {
    560.0
}

fn default_window_height() -> f32 {
    700.0
}

fn default_voiceger_url() -> String {
    "http://localhost:9880".to_string()
}

fn default_voiceger_prompt_lang() -> String {
    "ja".to_string()
}

fn default_active_engine_id() -> String {
    ENGINE_ID_VOICEVOX.to_string()
}

fn default_engine_params() -> JsonValue {
    JsonValue::Object(JsonMap::new())
}

fn default_generic_url() -> String {
    "http://localhost:10101".to_string()
}

fn default_generic_engine_name() -> String {
    "その他".to_string()
}

fn default_generic_engines() -> Vec<GenericEngineConfig> {
    vec![GenericEngineConfig {
        name: default_generic_engine_name(),
        url: default_generic_url(),
    }]
}

impl Default for SynthParamsConfig {
    fn default() -> Self {
        Self {
            speed_scale: 1.0,
            pitch_scale: 0.0,
            intonation_scale: 1.0,
            volume_scale: 1.0,
            engine_params: default_engine_params(),
        }
    }
}

impl SynthParamsConfig {
    fn ensure_engine_params_object(&mut self) {
        if !self.engine_params.is_object() {
            self.engine_params = default_engine_params();
        }
    }

    fn engine_params_object_mut(&mut self) -> &mut JsonMap<String, JsonValue> {
        self.ensure_engine_params_object();
        self.engine_params
            .as_object_mut()
            .expect("engine_params must be object")
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            voicevox_url: validation::DEFAULT_VOICEVOX_URL.to_string(),
            voicevox_path: String::new(),
            auto_launch_voicevox: false,
            auto_launch_voiceger: false,
            auto_start_app: false,
            synth_params: SynthParamsConfig::default(),
            speaker_id: 3, // ずんだもん (ノーマル)
            monitor_audio: true,
            virtual_device_name: validation::DEFAULT_DEVICE_NAME.to_string(),
            templates: vec![
                "こんにちは！".to_string(),
                "ありがとう！".to_string(),
                "おつかれさまなのだ！".to_string(),
                "了解なのだ！".to_string(),
            ],
            osc_enabled: false,
            osc_address: "127.0.0.1".to_string(),
            osc_port: 9000,
            soundboard_path: ProjectDirs::from("", "", "zundux_tts")
                .map(|d| d.config_dir().join("sounds").to_string_lossy().to_string())
                .unwrap_or_else(|| "sounds".to_string()),
            mic_source_name: None,
            echo_enabled: false,
            echo_delay_ms: 200,
            echo_decay: 0.4,
            effects_tts_only: false,
            use_as_default_input: false,
            target_lufs: -14.0,
            loudness_tolerance: 3.0,
            soundboard_gains: std::collections::HashMap::new(),
            noise_suppression: false,
            silent_words: Vec::new(),
            theme: Theme::default(),
            presets: Self::default_presets(),
            templates_default_expanded: false,
            window_width: 560.0,
            window_height: 700.0,
            active_engine: TtsEngineType::Voicevox,
            active_engine_id: default_active_engine_id(),
            engine_configs: std::collections::HashMap::new(),
            voiceger_url: "http://localhost:9880".to_string(),
            voiceger_path: String::new(),
            voiceger_ref_audio: String::new(),
            voiceger_prompt_text: String::new(),
            voiceger_prompt_lang: "ja".to_string(),
            voiceger_ref_free: false,
            voiceger_dict: std::collections::HashMap::new(), // per-lang dicts initialized on demand
            generic_engines: default_generic_engines(),
            active_generic_engine_idx: 0,
            generic_speaker_id: 0,
            generic_synth_params: SynthParamsConfig::default(),
            generic_url_legacy: String::new(),
            generic_engine_name_legacy: String::new(),
        }
    }
}

impl AppConfig {
    /// Default Voiceger install directory (~/voiceger_v2, matching install.sh).
    pub fn default_voiceger_install_dir() -> PathBuf {
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("voiceger_v2")
    }

    /// Build the default launch command from the standard install.sh layout.
    /// Uses `conda run` if conda is found, otherwise falls back to plain python.
    pub fn default_voiceger_launch_cmd() -> String {
        let api_py = Self::default_voiceger_install_dir()
            .join("GPT-SoVITS")
            .join("api_v2.py");

        // Look for conda in common locations
        let home = std::env::var("HOME").unwrap_or_default();
        let miniconda = format!("{home}/miniconda3/bin/conda");
        let anaconda = format!("{home}/anaconda3/bin/conda");
        let conda_candidates = ["conda", miniconda.as_str(), anaconda.as_str()];
        for candidate in &conda_candidates {
            if std::process::Command::new(candidate)
                .arg("--version")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
            {
                return format!(
                    "{} run -n voiceger --no-capture-output python {}",
                    candidate,
                    api_py.display()
                );
            }
        }

        // Fallback: plain python in PATH
        format!("python {}", api_py.display())
    }

    /// The effective launch command: user-set value, or the computed default.
    pub fn effective_voiceger_launch_cmd(&self) -> String {
        if self.voiceger_path.trim().is_empty() {
            Self::default_voiceger_launch_cmd()
        } else {
            self.voiceger_path.clone()
        }
    }

    /// Derive the Voiceger repository root from voiceger_path.
    /// e.g. "python /home/user/voiceger_v2/api.py" → "/home/user/voiceger_v2"
    /// Falls back to the default install directory when voiceger_path is empty.
    pub fn voiceger_base_dir(&self) -> Option<PathBuf> {
        if self.voiceger_path.trim().is_empty() {
            let dir = Self::default_voiceger_install_dir();
            return if dir.exists() { Some(dir) } else { None };
        }
        let words = shell_words::split(self.voiceger_path.trim()).ok()?;
        // Prefer a .py script argument over any other path-like word.
        // e.g. "python /path/to/api_v2.py" → pick api_v2.py, not the python binary.
        let script = words
            .iter()
            .find(|w| w.ends_with(".py"))
            .or_else(|| words.iter().find(|w| w.contains('/')))?;
        let parent = PathBuf::from(script).parent().map(|p| p.to_path_buf())?;
        // api_v2.py is inside GPT-SoVITS/, so the repo root is one level up
        parent.parent().map(|p| p.to_path_buf()).or(Some(parent))
    }

    /// Default ref audio path derived from voiceger_path.
    pub fn default_voiceger_ref_audio(&self) -> String {
        self.voiceger_base_dir()
            .map(|d| {
                d.join("reference")
                    .join("01_ref_emoNormal026.wav")
                    .to_string_lossy()
                    .to_string()
            })
            .unwrap_or_default()
    }

    pub const DEFAULT_VOICEGER_PROMPT_LANG: &'static str = "ja";

    /// Read prompt text from ref_text.txt in the reference folder, falling back to the known default.
    pub fn default_voiceger_prompt_text(&self) -> String {
        if let Some(base) = self.voiceger_base_dir() {
            let path = base.join("reference").join("ref_text.txt");
            if let Ok(text) = std::fs::read_to_string(&path) {
                let trimmed = text.trim().to_string();
                if !trimmed.is_empty() {
                    return trimmed;
                }
            }
        }
        // All _026.wav reference files contain JSUT dataset utterance #026
        "私はいつもミネラルウォーターを持ち歩いています".to_string()
    }

    /// Apply all Voiceger defaults derived from voiceger_path.
    pub fn reset_voiceger_defaults(&mut self) {
        self.voiceger_ref_audio = self.default_voiceger_ref_audio();
        self.voiceger_prompt_text = self.default_voiceger_prompt_text();
        self.voiceger_prompt_lang = Self::DEFAULT_VOICEGER_PROMPT_LANG.to_string();
    }

    pub fn default_presets() -> Vec<SpeakerPreset> {
        vec![
            SpeakerPreset {
                name: "デフォルト：ずんだもん".to_string(),
                speaker_id: 3,
                synth_params: SynthParamsConfig::default(),
                engine: TtsEngineType::Voicevox,
                engine_id: ENGINE_ID_VOICEVOX.to_string(),
                voiceger_emotion: String::new(),
                voiceger_ref_audio_override: String::new(),
                generic_engine_name: String::new(),
            },
            SpeakerPreset {
                name: "デフォルト：めたん".to_string(),
                speaker_id: 2,
                synth_params: SynthParamsConfig::default(),
                engine: TtsEngineType::Voicevox,
                engine_id: ENGINE_ID_VOICEVOX.to_string(),
                voiceger_emotion: String::new(),
                voiceger_ref_audio_override: String::new(),
                generic_engine_name: String::new(),
            },
            SpeakerPreset {
                name: "デフォルト：つむぎ".to_string(),
                speaker_id: 8,
                synth_params: SynthParamsConfig::default(),
                engine: TtsEngineType::Voicevox,
                engine_id: ENGINE_ID_VOICEVOX.to_string(),
                voiceger_emotion: String::new(),
                voiceger_ref_audio_override: String::new(),
                generic_engine_name: String::new(),
            },
            SpeakerPreset {
                name: "Voiceger：日本語".to_string(),
                speaker_id: 0,
                synth_params: SynthParamsConfig::default(),
                engine: TtsEngineType::Voiceger,
                engine_id: ENGINE_ID_VOICEGER.to_string(),
                voiceger_emotion: String::new(),
                voiceger_ref_audio_override: String::new(),
                generic_engine_name: String::new(),
            },
            SpeakerPreset {
                name: "Voiceger：English".to_string(),
                speaker_id: 1,
                synth_params: SynthParamsConfig::default(),
                engine: TtsEngineType::Voiceger,
                engine_id: ENGINE_ID_VOICEGER.to_string(),
                voiceger_emotion: String::new(),
                voiceger_ref_audio_override: String::new(),
                generic_engine_name: String::new(),
            },
            SpeakerPreset {
                name: "Voiceger：中文".to_string(),
                speaker_id: 2,
                synth_params: SynthParamsConfig::default(),
                engine: TtsEngineType::Voiceger,
                engine_id: ENGINE_ID_VOICEGER.to_string(),
                voiceger_emotion: String::new(),
                voiceger_ref_audio_override: String::new(),
                generic_engine_name: String::new(),
            },
            SpeakerPreset {
                name: "Voiceger：한국어".to_string(),
                speaker_id: 3,
                synth_params: SynthParamsConfig::default(),
                engine: TtsEngineType::Voiceger,
                engine_id: ENGINE_ID_VOICEGER.to_string(),
                voiceger_emotion: String::new(),
                voiceger_ref_audio_override: String::new(),
                generic_engine_name: String::new(),
            },
            SpeakerPreset {
                name: "Voiceger：粤語".to_string(),
                speaker_id: 4,
                synth_params: SynthParamsConfig::default(),
                engine: TtsEngineType::Voiceger,
                engine_id: ENGINE_ID_VOICEGER.to_string(),
                voiceger_emotion: String::new(),
                voiceger_ref_audio_override: String::new(),
                generic_engine_name: String::new(),
            },
        ]
    }

    fn config_dir() -> Result<PathBuf> {
        let dirs = ProjectDirs::from("", "", "zundux_tts")
            .context("Failed to determine config directory")?;
        Ok(dirs.config_dir().to_path_buf())
    }

    fn config_path() -> Result<PathBuf> {
        Ok(Self::config_dir()?.join("config.toml"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            let config = Self::default();
            config.save()?;
            return Ok(config);
        }
        validation::check_config_file_size(&path)?;
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config: {}", path.display()))?;
        let mut config: Self =
            toml::from_str(&content).with_context(|| "Failed to parse config TOML")?;
        config.validate_and_sanitize_with(&Self::top_level_keys(&content));
        Ok(config)
    }

    fn sync_active_engine_compat_fields(&mut self) {
        let active_engine_id = self.active_engine_id.trim();
        if active_engine_id.is_empty() {
            self.active_engine_id = self.active_engine.as_engine_id().to_string();
            return;
        }

        if let Some(engine) = TtsEngineType::from_engine_id(active_engine_id) {
            self.active_engine = engine;
            if engine != TtsEngineType::Generic {
                self.active_engine_id = engine.as_engine_id().to_string();
            }
        }
    }

    /// Keys set directly at the top level of a config TOML file.
    fn top_level_keys(content: &str) -> HashSet<String> {
        content
            .parse::<toml::Table>()
            .map(|t| t.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Copy `[engine_configs.*]` values into the legacy flat fields.
    ///
    /// A legacy field that is written explicitly at the top level of the file
    /// (e.g. `voiceger_path` set by install.sh or a manual edit) wins over the
    /// mirrored value in `engine_configs`. Otherwise a stale `path = ""` there
    /// silently discards the real launch command and Voiceger can no longer start.
    fn apply_engine_configs_to_legacy(&mut self, top_level_keys: &HashSet<String>) {
        let use_mirror = |legacy_key: &str| !top_level_keys.contains(legacy_key);

        if let Some(voicevox) = self
            .engine_configs
            .get(ENGINE_ID_VOICEVOX)
            .and_then(JsonValue::as_object)
            .cloned()
        {
            if use_mirror("voicevox_url") {
                if let Some(url) = voicevox.get("url").and_then(JsonValue::as_str) {
                    self.voicevox_url = url.to_string();
                }
            }
            if use_mirror("voicevox_path") {
                if let Some(path) = voicevox.get("path").and_then(JsonValue::as_str) {
                    self.voicevox_path = path.to_string();
                }
            }
            if use_mirror("auto_launch_voicevox") {
                if let Some(v) = voicevox.get("auto_launch").and_then(JsonValue::as_bool) {
                    self.auto_launch_voicevox = v;
                }
            }
        }

        if let Some(voiceger) = self
            .engine_configs
            .get(ENGINE_ID_VOICEGER)
            .and_then(JsonValue::as_object)
            .cloned()
        {
            if use_mirror("voiceger_url") {
                if let Some(url) = voiceger.get("url").and_then(JsonValue::as_str) {
                    self.voiceger_url = url.to_string();
                }
            }
            if use_mirror("voiceger_path") {
                if let Some(path) = voiceger.get("path").and_then(JsonValue::as_str) {
                    self.voiceger_path = path.to_string();
                }
            }
            if use_mirror("auto_launch_voiceger") {
                if let Some(v) = voiceger.get("auto_launch").and_then(JsonValue::as_bool) {
                    self.auto_launch_voiceger = v;
                }
            }
            if use_mirror("voiceger_ref_audio") {
                if let Some(v) = voiceger.get("ref_audio").and_then(JsonValue::as_str) {
                    self.voiceger_ref_audio = v.to_string();
                }
            }
            if use_mirror("voiceger_prompt_text") {
                if let Some(v) = voiceger.get("prompt_text").and_then(JsonValue::as_str) {
                    self.voiceger_prompt_text = v.to_string();
                }
            }
            if use_mirror("voiceger_prompt_lang") {
                if let Some(v) = voiceger.get("prompt_lang").and_then(JsonValue::as_str) {
                    self.voiceger_prompt_lang = v.to_string();
                }
            }
            if use_mirror("voiceger_ref_free") {
                if let Some(v) = voiceger.get("ref_free").and_then(JsonValue::as_bool) {
                    self.voiceger_ref_free = v;
                }
            }
            if use_mirror("voiceger_dict") {
                if let Some(dict) = voiceger.get("dict") {
                    if let Ok(dict) = serde_json::from_value::<
                        std::collections::HashMap<
                            String,
                            std::collections::HashMap<String, String>,
                        >,
                    >(dict.clone())
                    {
                        self.voiceger_dict = dict;
                    }
                }
            }
        }

        if let Some(generic) = self
            .engine_configs
            .get(ENGINE_ID_GENERIC)
            .and_then(JsonValue::as_object)
            .cloned()
        {
            if use_mirror("generic_engines") {
                if let Some(engines) = generic.get("engines") {
                    if let Ok(engines) =
                        serde_json::from_value::<Vec<GenericEngineConfig>>(engines.clone())
                    {
                        self.generic_engines = engines;
                    }
                }
            }
            if use_mirror("active_generic_engine_idx") {
                if let Some(idx) = generic.get("active_engine_idx").and_then(JsonValue::as_u64) {
                    self.active_generic_engine_idx = idx as usize;
                }
            }
            if use_mirror("generic_speaker_id") {
                if let Some(id) = generic.get("speaker_id").and_then(JsonValue::as_u64) {
                    self.generic_speaker_id = id as u32;
                }
            }
            if use_mirror("generic_synth_params") {
                if let Some(synth_params) = generic.get("synth_params") {
                    if let Ok(synth_params) =
                        serde_json::from_value::<SynthParamsConfig>(synth_params.clone())
                    {
                        self.generic_synth_params = synth_params;
                    }
                }
            }
        }
    }

    fn merge_engine_config_object(&mut self, engine_id: &str, updates: JsonMap<String, JsonValue>) {
        let entry = self
            .engine_configs
            .entry(engine_id.to_string())
            .or_insert_with(default_engine_params);
        if !entry.is_object() {
            *entry = default_engine_params();
        }
        let entry_object = entry
            .as_object_mut()
            .expect("engine config entry must be JSON object");
        for (key, value) in updates {
            entry_object.insert(key, value);
        }
    }

    fn sync_engine_configs_from_legacy(&mut self) {
        let mut voicevox = JsonMap::new();
        voicevox.insert(
            "url".to_string(),
            JsonValue::String(self.voicevox_url.clone()),
        );
        voicevox.insert(
            "path".to_string(),
            JsonValue::String(self.voicevox_path.clone()),
        );
        voicevox.insert(
            "auto_launch".to_string(),
            JsonValue::Bool(self.auto_launch_voicevox),
        );
        self.merge_engine_config_object(ENGINE_ID_VOICEVOX, voicevox);

        let mut voiceger = JsonMap::new();
        voiceger.insert(
            "url".to_string(),
            JsonValue::String(self.voiceger_url.clone()),
        );
        voiceger.insert(
            "path".to_string(),
            JsonValue::String(self.voiceger_path.clone()),
        );
        voiceger.insert(
            "auto_launch".to_string(),
            JsonValue::Bool(self.auto_launch_voiceger),
        );
        voiceger.insert(
            "ref_audio".to_string(),
            JsonValue::String(self.voiceger_ref_audio.clone()),
        );
        voiceger.insert(
            "prompt_text".to_string(),
            JsonValue::String(self.voiceger_prompt_text.clone()),
        );
        voiceger.insert(
            "prompt_lang".to_string(),
            JsonValue::String(self.voiceger_prompt_lang.clone()),
        );
        voiceger.insert(
            "ref_free".to_string(),
            JsonValue::Bool(self.voiceger_ref_free),
        );
        voiceger.insert(
            "dict".to_string(),
            serde_json::to_value(&self.voiceger_dict).unwrap_or_else(|_| default_engine_params()),
        );
        self.merge_engine_config_object(ENGINE_ID_VOICEGER, voiceger);

        let mut generic = JsonMap::new();
        generic.insert(
            "engines".to_string(),
            serde_json::to_value(&self.generic_engines)
                .unwrap_or_else(|_| JsonValue::Array(Vec::new())),
        );
        generic.insert(
            "active_engine_idx".to_string(),
            JsonValue::Number((self.active_generic_engine_idx as u64).into()),
        );
        generic.insert(
            "speaker_id".to_string(),
            JsonValue::Number((self.generic_speaker_id as u64).into()),
        );
        generic.insert(
            "synth_params".to_string(),
            serde_json::to_value(&self.generic_synth_params)
                .unwrap_or_else(|_| default_engine_params()),
        );
        self.merge_engine_config_object(ENGINE_ID_GENERIC, generic);
    }

    fn sync_generalized_fields_for_save(&mut self) {
        self.synth_params.ensure_engine_params_object();
        self.generic_synth_params.ensure_engine_params_object();
        self.sync_active_engine_compat_fields();
        for preset in &mut self.presets {
            preset.sync_engine_compat_fields();
            preset.sync_legacy_preset_engine_params();
        }
        self.sync_engine_configs_from_legacy();
    }

    #[cfg(test)]
    fn validate_and_sanitize(&mut self) {
        self.validate_and_sanitize_with(&HashSet::new());
    }

    fn validate_and_sanitize_with(&mut self, top_level_keys: &HashSet<String>) {
        self.sync_active_engine_compat_fields();
        self.apply_engine_configs_to_legacy(top_level_keys);

        if !validation::is_valid_device_name(&self.virtual_device_name) {
            tracing::warn!(
                "Invalid virtual_device_name '{}', using default",
                self.virtual_device_name
            );
            self.virtual_device_name = validation::DEFAULT_DEVICE_NAME.to_string();
        }

        if let Err(e) = validation::is_valid_voicevox_url(&self.voicevox_url) {
            tracing::warn!("Invalid voicevox_url: {}, using default", e);
            self.voicevox_url = validation::DEFAULT_VOICEVOX_URL.to_string();
        }

        if self.templates.len() > 100 {
            tracing::warn!(
                "Too many templates ({}), truncating to 100",
                self.templates.len()
            );
            self.templates.truncate(100);
        }
        for t in &mut self.templates {
            if t.len() > 512 {
                tracing::warn!("Template too long, truncating to 512 chars");
                *t = t.chars().take(512).collect();
            }
        }

        self.theme = std::mem::take(&mut self.theme).validated();

        // Migrate: 旧 generic_url / generic_engine_name → generic_engines リスト
        if self.generic_engines.is_empty() {
            let name = if self.generic_engine_name_legacy.is_empty() {
                default_generic_engine_name()
            } else {
                std::mem::take(&mut self.generic_engine_name_legacy)
            };
            let url = if self.generic_url_legacy.is_empty() {
                default_generic_url()
            } else {
                std::mem::take(&mut self.generic_url_legacy)
            };
            self.generic_engines.push(GenericEngineConfig { name, url });
        }
        // 上限チェック
        if self.generic_engines.len() > 20 {
            self.generic_engines.truncate(20);
        }
        // インデックスが範囲外の場合は先頭に戻す
        if self.active_generic_engine_idx >= self.generic_engines.len() {
            self.active_generic_engine_idx = 0;
        }

        // Migrate: if no presets exist, add the three named defaults.
        if self.presets.is_empty() {
            self.presets = Self::default_presets();
        }
        if self.presets.len() > 50 {
            self.presets.truncate(50);
        }
        self.synth_params.ensure_engine_params_object();
        self.generic_synth_params.ensure_engine_params_object();
        for p in &mut self.presets {
            if p.name.len() > 64 {
                p.name = p.name.chars().take(64).collect();
            }
            p.synth_params.ensure_engine_params_object();
            p.sync_engine_compat_fields();
            p.sync_legacy_preset_engine_params();
        }
        self.sync_active_engine_compat_fields();
        self.sync_engine_configs_from_legacy();
    }

    /// アクティブな汎用エンジン設定を返す。
    pub fn active_generic_config(&self) -> Option<&GenericEngineConfig> {
        self.generic_engines.get(self.active_generic_engine_idx)
    }

    /// ステータスバー・UI に表示するアクティブエンジン名を返す。
    pub fn active_generic_name(&self) -> &str {
        self.active_generic_config()
            .map(|e| e.name.as_str())
            .unwrap_or("その他")
    }

    pub fn save(&self) -> Result<()> {
        let dir = Self::config_dir()?;
        std::fs::create_dir_all(&dir)?;
        let path = Self::config_path()?;
        let mut config_to_save = self.clone();
        config_to_save.sync_generalized_fields_for_save();
        let content = toml::to_string_pretty(&config_to_save)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    /// Returns the path to the XDG autostart .desktop file
    fn autostart_desktop_path() -> Result<PathBuf> {
        let home = std::env::var("HOME").context("HOME not set")?;
        Ok(PathBuf::from(home).join(".config/autostart/zundux_tts.desktop"))
    }

    /// Returns the path to the currently running executable
    fn current_exe_path() -> Result<String> {
        let exe = std::env::current_exe().context("Failed to get current exe path")?;
        Ok(exe.to_string_lossy().to_string())
    }

    /// Check if the autostart .desktop file exists
    pub fn is_autostart_enabled() -> bool {
        Self::autostart_desktop_path()
            .map(|p| p.exists())
            .unwrap_or(false)
    }

    /// Install or remove the autostart .desktop entry
    pub fn set_autostart(enabled: bool) -> Result<()> {
        let desktop_path = Self::autostart_desktop_path()?;

        if enabled {
            let exe_path = Self::current_exe_path()?;
            let autostart_dir = desktop_path.parent().context("No parent dir")?;
            std::fs::create_dir_all(autostart_dir)?;

            let content = format!(
                "[Desktop Entry]\n\
                 Type=Application\n\
                 Name=ZunduxTTS\n\
                 Comment=VOICEVOX TTS virtual microphone\n\
                 Exec={exe_path}\n\
                 Terminal=false\n\
                 X-GNOME-Autostart-enabled=true\n"
            );
            std::fs::write(&desktop_path, content)
                .with_context(|| format!("Failed to write {}", desktop_path.display()))?;
            tracing::info!("Autostart enabled: {}", desktop_path.display());
        } else if desktop_path.exists() {
            std::fs::remove_file(&desktop_path)
                .with_context(|| format!("Failed to remove {}", desktop_path.display()))?;
            tracing::info!("Autostart disabled: removed {}", desktop_path.display());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_legacy_engine_fields_to_generalized() {
        let raw = r#"
active_engine = "voiceger"

[[presets]]
name = "legacy-generic"
speaker_id = 7
engine = "generic"
generic_engine_name = "custom-engine"
voiceger_emotion = "甘え"
voiceger_ref_audio_override = "/home/user/ref.wav"

[presets.synth_params]
speed_scale = 1.0
pitch_scale = 0.0
intonation_scale = 1.0
volume_scale = 1.0
"#;

        let mut config: AppConfig = toml::from_str(raw).expect("parse legacy toml");
        config.validate_and_sanitize();

        assert_eq!(config.active_engine_id, ENGINE_ID_VOICEGER);
        let preset = &config.presets[0];
        assert_eq!(preset.engine_id, "generic:custom-engine");
        assert_eq!(preset.engine, TtsEngineType::Generic);
        assert_eq!(
            preset
                .synth_params
                .engine_params
                .get("voiceger_emotion")
                .and_then(JsonValue::as_str),
            Some("甘え")
        );
    }

    #[test]
    fn applies_generalized_engine_configs_to_legacy_fields() {
        let raw = r#"
active_engine_id = "voicevox"

[engine_configs.voicevox]
url = "http://127.0.0.1:50031"
path = "/opt/voicevox/run"
auto_launch = true

[engine_configs.voiceger]
url = "http://localhost:19999"
path = "/opt/voiceger/run"
auto_launch = true
ref_audio = "/home/user/ref.wav"
prompt_text = "hello"
prompt_lang = "en"
ref_free = true

[engine_configs.generic]
active_engine_idx = 0
speaker_id = 55
engines = [{ name = "other", url = "http://localhost:18000" }]
synth_params = { speed_scale = 1.1, pitch_scale = 0.1, intonation_scale = 1.2, volume_scale = 0.9, engine_params = { key = "value" } }
"#;

        let mut config: AppConfig = toml::from_str(raw).expect("parse generalized toml");
        config.validate_and_sanitize();

        assert_eq!(config.voicevox_url, "http://127.0.0.1:50031");
        assert_eq!(config.voiceger_url, "http://localhost:19999");
        assert_eq!(config.generic_speaker_id, 55);
        assert_eq!(config.generic_engines[0].name, "other");
        assert_eq!(config.generic_synth_params.speed_scale, 1.1);
        assert_eq!(
            config
                .generic_synth_params
                .engine_params
                .get("key")
                .and_then(JsonValue::as_str),
            Some("value")
        );
    }

    #[test]
    fn explicit_top_level_fields_win_over_stale_engine_configs() {
        // Reproduces a config where install.sh rewrote the top-level voiceger_path
        // while the app-maintained [engine_configs.voiceger] still had an empty path.
        let raw = r#"
voiceger_path = "/home/user/miniconda3/envs/voiceger/bin/python /home/user/voiceger_v2/GPT-SoVITS/api_v2.py"
voiceger_ref_audio = "/home/user/voiceger_v2/reference/01_ref_emoNormal026.wav"

[engine_configs.voiceger]
url = "http://localhost:19999"
path = ""
ref_audio = ""
auto_launch = true
"#;

        let mut config: AppConfig = toml::from_str(raw).expect("parse toml");
        config.validate_and_sanitize_with(&AppConfig::top_level_keys(raw));

        assert_eq!(
            config.voiceger_path,
            "/home/user/miniconda3/envs/voiceger/bin/python /home/user/voiceger_v2/GPT-SoVITS/api_v2.py"
        );
        assert_eq!(
            config.voiceger_ref_audio,
            "/home/user/voiceger_v2/reference/01_ref_emoNormal026.wav"
        );
        // Values not set at the top level still come from engine_configs.
        assert_eq!(config.voiceger_url, "http://localhost:19999");
        assert!(config.auto_launch_voiceger);
        // And the mirror is brought back in sync for the next save.
        assert_eq!(
            config
                .engine_configs
                .get(ENGINE_ID_VOICEGER)
                .and_then(|v| v.get("path"))
                .and_then(JsonValue::as_str),
            Some(config.voiceger_path.as_str())
        );
    }

    #[test]
    fn keeps_unknown_generalized_engine_data() {
        let mut config = AppConfig::default();
        config.engine_configs.insert(
            "custom".to_string(),
            serde_json::json!({"enabled": true, "rate": 42}),
        );
        config.engine_configs.insert(
            ENGINE_ID_VOICEVOX.to_string(),
            serde_json::json!({"custom_field": "keep-me"}),
        );

        config.validate_and_sanitize();

        assert_eq!(
            config
                .engine_configs
                .get("custom")
                .and_then(JsonValue::as_object)
                .and_then(|o| o.get("enabled"))
                .and_then(JsonValue::as_bool),
            Some(true)
        );
        assert_eq!(
            config
                .engine_configs
                .get(ENGINE_ID_VOICEVOX)
                .and_then(JsonValue::as_object)
                .and_then(|o| o.get("custom_field"))
                .and_then(JsonValue::as_str),
            Some("keep-me")
        );
    }
}
