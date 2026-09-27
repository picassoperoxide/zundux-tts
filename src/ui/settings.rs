use crate::app::AppState;
use crate::config::{AppConfig, GenericEngineConfig, TtsEngineType, ENGINE_ID_GENERIC};
use crate::tts::voiceger::VOICEGER_LANGUAGES;
use crate::ui::widgets;
use crate::validation;

const PRESET_REF_WAV_BUTTONS_WIDTH: f32 = 120.0;

/// Render the preset list + editor for one engine group.
/// Call this from inside a `ui.collapsing(...)` closure.
/// `generic_engine_name` が Some の場合、Genericプリセットをそのエンジン名でフィルタする。
fn show_preset_section(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine_group: &TtsEngineType,
    generic_engine_name: Option<&str>,
) {
    let editing_this = (state.preset_adding || state.preset_edit_idx.is_some())
        && state
            .preset_edit_buf
            .as_ref()
            .map(|b| &b.engine == engine_group)
            .unwrap_or(false);

    let mut save_clicked = false;
    let mut cancel_clicked = false;

    if editing_this {
        let speakers_snapshot = match engine_group {
            TtsEngineType::Generic => state.generic_speakers.clone(),
            _ => state.speakers.clone(),
        };
        if let Some(buf) = state.preset_edit_buf.as_mut() {
            ui.horizontal(|ui| {
                ui.label("名前:");
                ui.add(
                    egui::TextEdit::singleline(&mut buf.name).desired_width(ui.available_width()),
                );
            });
            ui.add_space(4.0);

            let combo_id = match engine_group {
                TtsEngineType::Voicevox => "preset_edit_speaker_vox",
                TtsEngineType::Voiceger => "preset_edit_speaker_vgr",
                TtsEngineType::Generic => "preset_edit_speaker_gen",
            };
            let speaker_text = speakers_snapshot
                .iter()
                .flat_map(|s| s.styles.iter().map(move |st| (s, st)))
                .find(|(_, st)| st.id == buf.speaker_id)
                .map(|(s, st)| format!("{} - {}", s.name, st.name))
                .unwrap_or_else(|| format!("Speaker ID: {}", buf.speaker_id));

            ui.horizontal(|ui| {
                ui.label("スピーカー:");
                egui::ComboBox::from_id_salt(combo_id)
                    .selected_text(&speaker_text)
                    .show_ui(ui, |ui| {
                        for speaker in &speakers_snapshot {
                            for style in &speaker.styles {
                                let label = format!("{} - {}", speaker.name, style.name);
                                ui.selectable_value(&mut buf.speaker_id, style.id, &label);
                            }
                        }
                    });
            });
            ui.add_space(4.0);

            // Emotion selector (Voiceger only)
            if engine_group == &TtsEngineType::Voiceger {
                ui.horizontal(|ui| {
                    ui.label("感情:");
                    let emotion_label = if buf.voiceger_emotion.is_empty() {
                        "ノーマル"
                    } else {
                        buf.voiceger_emotion.as_str()
                    };
                    egui::ComboBox::from_id_salt("preset_edit_emotion_vgr")
                        .selected_text(emotion_label)
                        .show_ui(ui, |ui| {
                            for (name, _) in crate::tts::voiceger::VOICEGER_EMOTIONS {
                                ui.selectable_value(
                                    &mut buf.voiceger_emotion,
                                    name.to_string(),
                                    *name,
                                );
                            }
                        });
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("参照音声(任意):");
                    ui.add(
                        egui::TextEdit::singleline(&mut buf.voiceger_ref_audio_override)
                            .desired_width(ui.available_width() - PRESET_REF_WAV_BUTTONS_WIDTH)
                            .hint_text("空欄 = 感情/グローバル参照音声を使用"),
                    );
                    if ui.small_button("参照").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter(
                                "音声ファイル",
                                &["wav", "flac", "mp3", "ogg", "m4a", "aac"],
                            )
                            .pick_file()
                        {
                            buf.voiceger_ref_audio_override = path.to_string_lossy().to_string();
                        }
                    }
                    if ui.small_button("クリア").clicked() {
                        buf.voiceger_ref_audio_override.clear();
                    }
                });
                ui.label(
                    egui::RichText::new(
                        "※ 設定時は「参照音声 > 感情 > グローバル参照音声」の順で優先されます",
                    )
                    .small()
                    .weak(),
                );
                ui.label(
                    egui::RichText::new("⚠ WAV・FLACを推奨。MP3・AACは非可逆圧縮のため高周波が欠落し、声質の再現精度が下がる場合があります。")
                        .small()
                        .weak(),
                );
                ui.add_space(4.0);
            }

            ui.horizontal(|ui| {
                ui.label("速度:");
                ui.add(
                    egui::Slider::new(&mut buf.synth_params.speed_scale, 0.5..=2.0).step_by(0.05),
                );
            });
            ui.horizontal(|ui| {
                ui.label("ピッチ:");
                ui.add(
                    egui::Slider::new(&mut buf.synth_params.pitch_scale, -0.15..=0.15)
                        .step_by(0.01),
                );
            });
            ui.horizontal(|ui| {
                ui.label("抑揚:");
                ui.add(
                    egui::Slider::new(&mut buf.synth_params.intonation_scale, 0.0..=2.0)
                        .step_by(0.05),
                );
            });
            ui.horizontal(|ui| {
                ui.label("音量:");
                ui.add(
                    egui::Slider::new(&mut buf.synth_params.volume_scale, 0.0..=2.0).step_by(0.05),
                );
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                save_clicked = ui.button("保存").clicked();
                cancel_clicked = ui.button("キャンセル").clicked();
            });
        }
        ui.separator();
    }

    if save_clicked {
        if let Some(preset) = state.preset_edit_buf.take() {
            if !preset.name.trim().is_empty() {
                if state.preset_adding {
                    state.config.presets.push(preset);
                } else if let Some(idx) = state.preset_edit_idx {
                    if idx < state.config.presets.len() {
                        state.config.presets[idx] = preset;
                    }
                }
                let _ = state.config.save();
            }
        }
        state.preset_adding = false;
        state.preset_edit_idx = None;
    }
    if cancel_clicked {
        state.preset_adding = false;
        state.preset_edit_idx = None;
        state.preset_edit_buf = None;
    }

    // --- Preset list ---
    let mut to_edit: Option<usize> = None;
    let mut to_delete: Option<usize> = None;

    let group_indices: Vec<usize> = (0..state.config.presets.len())
        .filter(|&i| {
            let p = &state.config.presets[i];
            if &p.engine != engine_group {
                return false;
            }
            // Genericエンジンの場合、エンジン名でもフィルタ
            if let Some(name) = generic_engine_name {
                return p.generic_engine_name.is_empty() || p.generic_engine_name == name;
            }
            true
        })
        .collect();

    for i in group_indices {
        let preset = &state.config.presets[i];
        ui.horizontal(|ui| {
            let active = state.active_preset_idx == Some(i);
            if active {
                ui.label(
                    egui::RichText::new("▶")
                        .color(state.config.theme.color(state.config.theme.status_ok)),
                );
            } else {
                ui.label("　");
            }
            ui.label(&preset.name);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("削除").clicked() {
                    to_delete = Some(i);
                }
                if ui.small_button("編集").clicked() {
                    to_edit = Some(i);
                }
            });
        });
    }

    if let Some(i) = to_edit {
        if state.preset_edit_idx != Some(i) {
            state.preset_edit_idx = Some(i);
            state.preset_adding = false;
            state.preset_edit_buf = Some(state.config.presets[i].clone());
        }
    }
    if let Some(i) = to_delete {
        state.config.presets.remove(i);
        if state.active_preset_idx == Some(i) {
            state.active_preset_idx = None;
        } else if let Some(idx) = state.active_preset_idx {
            if idx > i {
                state.active_preset_idx = Some(idx - 1);
            }
        }
        if state.preset_edit_idx == Some(i) {
            state.preset_edit_idx = None;
            state.preset_edit_buf = None;
        }
        let _ = state.config.save();
    }

    ui.add_space(4.0);
    if !editing_this && !state.preset_adding && state.preset_edit_idx.is_none() {
        ui.horizontal(|ui| {
            if ui.button("＋ 新規作成").clicked() {
                state.preset_adding = true;
                state.preset_edit_idx = None;
                let default_speaker_id = match engine_group {
                    TtsEngineType::Voicevox => state.config.speaker_id,
                    TtsEngineType::Voiceger => 0,
                    TtsEngineType::Generic => state.config.generic_speaker_id,
                };
                let engine_id = match engine_group {
                    TtsEngineType::Generic => {
                        if let Some(name) = generic_engine_name.filter(|name| !name.is_empty()) {
                            format!("{ENGINE_ID_GENERIC}:{name}")
                        } else {
                            ENGINE_ID_GENERIC.to_string()
                        }
                    }
                    _ => engine_group.as_engine_id().to_string(),
                };
                state.preset_edit_buf = Some(crate::config::SpeakerPreset {
                    name: String::new(),
                    speaker_id: default_speaker_id,
                    synth_params: match engine_group {
                        TtsEngineType::Generic => state.config.generic_synth_params.clone(),
                        _ => state.config.synth_params.clone(),
                    },
                    engine: engine_group.clone(),
                    engine_id,
                    voiceger_emotion: String::new(),
                    voiceger_ref_audio_override: String::new(),
                    generic_engine_name: generic_engine_name.unwrap_or("").to_string(),
                });
            }
            if engine_group == &TtsEngineType::Voicevox {
                if ui.button("デフォルトに戻す").clicked() {
                    state.config.presets = crate::config::AppConfig::default_presets();
                    state.active_preset_idx = None;
                    let _ = state.config.save();
                }
            }
        });
    }
}

fn engine_display_name(state: &AppState) -> String {
    match state.config.active_engine {
        TtsEngineType::Voicevox => "VOICEVOX".to_string(),
        TtsEngineType::Voiceger => "Voiceger".to_string(),
        TtsEngineType::Generic => state.config.active_generic_name().to_string(),
    }
}

/// Small bold heading used inside a section instead of another nested collapsing header.
fn sub_heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(egui::RichText::new(text).strong());
    ui.add_space(2.0);
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).small().weak());
}

/// "接続OK / 起動中... / 未接続" lamp.
fn connection_status(ui: &mut egui::Ui, state: &AppState, connected: bool, launching: bool) {
    let theme = &state.config.theme;
    if connected {
        ui.colored_label(theme.color(theme.status_ok), "● 接続OK");
    } else if launching {
        ui.colored_label(theme.color(theme.status_warn), "● 起動中...");
    } else {
        ui.colored_label(theme.color(theme.status_error), "● 未接続");
    }
}

/// Banner at the top of an engine section: either "使用中" or a button to switch to it.
fn engine_usage_banner(ui: &mut egui::Ui, state: &mut AppState, engine: TtsEngineType) {
    let is_active = state.config.active_engine == engine;
    ui.horizontal(|ui| {
        if is_active {
            ui.colored_label(
                state.config.theme.color(state.config.theme.status_ok),
                "✔ 現在このエンジンを使用中",
            );
        } else {
            ui.label(
                egui::RichText::new(format!("現在は {} を使用中", engine_display_name(state)))
                    .weak(),
            );
            if ui.button("このエンジンを使う").clicked() {
                state.pending_switch_engine = Some(engine);
            }
        }
    });
}

/// Engine picker at the top of the settings: VOICEVOX / Voiceger / その他.
fn show_engine_selector(ui: &mut egui::Ui, state: &mut AppState) {
    ui.label(egui::RichText::new("使用するTTSエンジン").strong());
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        let active = state.config.active_engine;
        if ui
            .radio(active == TtsEngineType::Voicevox, "VOICEVOX")
            .clicked()
            && active != TtsEngineType::Voicevox
        {
            state.pending_switch_engine = Some(TtsEngineType::Voicevox);
        }
        if ui
            .radio(active == TtsEngineType::Voiceger, "Voiceger")
            .clicked()
            && active != TtsEngineType::Voiceger
        {
            state.pending_switch_engine = Some(TtsEngineType::Voiceger);
        }

        let is_other = active == TtsEngineType::Generic;
        if state.config.generic_engines.is_empty() {
            ui.add_enabled(false, egui::RadioButton::new(false, "その他"))
                .on_disabled_hover_text("下の「その他のエンジン」から追加できます");
        } else {
            if ui.radio(is_other, "その他:").clicked() && !is_other {
                state.generic_connected = false;
                state.pending_switch_engine = Some(TtsEngineType::Generic);
            }
            let selected = state.config.active_generic_name().to_string();
            let mut picked: Option<usize> = None;
            egui::ComboBox::from_id_salt("other_engine_picker")
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    for (i, engine) in state.config.generic_engines.iter().enumerate() {
                        let is_this = is_other && state.config.active_generic_engine_idx == i;
                        if ui.selectable_label(is_this, &engine.name).clicked() && !is_this {
                            picked = Some(i);
                        }
                    }
                });
            if let Some(i) = picked {
                state.config.active_generic_engine_idx = i;
                state.generic_connected = false;
                state.pending_switch_engine = Some(TtsEngineType::Generic);
            }
        }
    });

    ui.add_space(4.0);
    let (connected, launching) = match state.config.active_engine {
        TtsEngineType::Voicevox => (state.voicevox_connected, state.voicevox_launching),
        TtsEngineType::Voiceger => (state.voiceger_connected, state.voiceger_launching),
        TtsEngineType::Generic => (state.generic_connected, false),
    };
    ui.horizontal(|ui| {
        ui.label(format!("{}:", engine_display_name(state)));
        connection_status(ui, state, connected, launching);
        match state.config.active_engine {
            TtsEngineType::Voicevox => {
                if ui.small_button("再起動").clicked() {
                    state.pending_restart_voicevox = true;
                }
            }
            TtsEngineType::Voiceger => {
                if ui.small_button("再起動").clicked() {
                    state.pending_restart_voiceger = true;
                }
            }
            TtsEngineType::Generic => {
                if ui.small_button("接続確認").clicked() {
                    state.generic_connected = false;
                    state.pending_health_check_generic = true;
                }
            }
        }
    });
}

fn show_voicevox_section(ui: &mut egui::Ui, state: &mut AppState) {
    engine_usage_banner(ui, state, TtsEngineType::Voicevox);

    sub_heading(ui, "接続・起動");
    ui.horizontal(|ui| {
        if ui.button("接続テスト").clicked() {
            state.pending_health_check = true;
        }
        if ui.button("起動").clicked() {
            state.pending_launch_voicevox = true;
        }
        connection_status(
            ui,
            state,
            state.voicevox_connected,
            state.voicevox_launching,
        );
    });
    ui.horizontal(|ui| {
        ui.label("URL:");
        ui.text_edit_singleline(&mut state.config.voicevox_url);
    });
    if validation::is_valid_voicevox_url(&state.config.voicevox_url).is_err() {
        ui.colored_label(
            state.config.theme.color(state.config.theme.status_warn),
            "URLはhttp://localhost または http://127.0.0.1 のみ",
        );
    }
    ui.horizontal(|ui| {
        ui.label("実行パス/コマンド:");
        ui.add(
            egui::TextEdit::singleline(&mut state.config.voicevox_path)
                .hint_text("空欄 = 起動済みのVOICEVOXにURLで接続"),
        );
        if ui.button("参照").clicked() {
            if let Some(path) = rfd::FileDialog::new().pick_file() {
                state.config.voicevox_path = path.to_string_lossy().to_string();
            }
        }
    });
    hint(
        ui,
        "空欄でも外部で起動済みのVOICEVOXにURL接続できます。起動/自動起動には実行パス/コマンドが必要です。",
    );
    let theme = state.config.theme.clone();
    if widgets::toggle(
        ui,
        &theme,
        &mut state.config.auto_launch_voicevox,
        "アプリ起動時にVOICEVOXを自動起動",
    )
    .changed()
    {
        let _ = state.config.save();
    }

    sub_heading(ui, "音声パラメータ（プリセット未選択時）");
    let params = &mut state.config.synth_params;
    ui.horizontal(|ui| {
        ui.label("速度:");
        ui.add(egui::Slider::new(&mut params.speed_scale, 0.5..=2.0).step_by(0.05));
    });
    ui.horizontal(|ui| {
        ui.label("ピッチ:");
        ui.add(egui::Slider::new(&mut params.pitch_scale, -0.15..=0.15).step_by(0.01));
    });
    ui.horizontal(|ui| {
        ui.label("抑揚:");
        ui.add(egui::Slider::new(&mut params.intonation_scale, 0.0..=2.0).step_by(0.05));
    });
    ui.horizontal(|ui| {
        ui.label("音量:");
        ui.add(egui::Slider::new(&mut params.volume_scale, 0.0..=2.0).step_by(0.05));
    });
    if ui.button("デフォルトに戻す").clicked() {
        *params = crate::config::SynthParamsConfig::default();
    }

    sub_heading(ui, "プリセット");
    show_preset_section(ui, state, &TtsEngineType::Voicevox, None);

    ui.add_space(4.0);
    ui.collapsing("VOICEVOX辞書", |ui| {
        if !state.voicevox_connected {
            ui.label("VOICEVOXに接続してください");
            return;
        }
        ui.horizontal(|ui| {
            if ui.button("辞書を読み込む").clicked() {
                state.pending_load_user_dict = true;
            }
            hint(ui, "(読み込んでいただくことで既存の割り当てを見られます)");
        });
        ui.add_space(4.0);

        let mut to_delete = None;
        for (uuid, surface, pronunciation) in &state.user_dict {
            ui.horizontal(|ui| {
                ui.label(format!("{} → {}", surface, pronunciation));
                if ui.small_button("削除").clicked() {
                    to_delete = Some(uuid.clone());
                }
            });
        }
        if let Some(uuid) = to_delete {
            state.pending_delete_dict_word = Some(uuid);
        }

        ui.add_space(4.0);
        ui.separator();

        ui.horizontal(|ui| {
            ui.label("表記:");
            ui.add(egui::TextEdit::singleline(&mut state.new_dict_surface).desired_width(100.0));
            ui.label("読み:");
            ui.add(
                egui::TextEdit::singleline(&mut state.new_dict_pronunciation).desired_width(100.0),
            );
            if ui.button("追加").clicked()
                && !state.new_dict_surface.trim().is_empty()
                && !state.new_dict_pronunciation.trim().is_empty()
            {
                let surface = state.new_dict_surface.trim().to_string();
                let pronunciation = state.new_dict_pronunciation.trim().to_string();
                state.pending_add_dict_word = Some((surface, pronunciation));
                state.new_dict_surface.clear();
                state.new_dict_pronunciation.clear();
            }
        });
    });
}

fn show_voiceger_section(ui: &mut egui::Ui, state: &mut AppState) {
    engine_usage_banner(ui, state, TtsEngineType::Voiceger);

    sub_heading(ui, "接続・起動");
    ui.horizontal(|ui| {
        if ui.button("起動").clicked() {
            state.pending_launch_voiceger = true;
        }
        if ui.button("再起動").clicked() {
            state.pending_restart_voiceger = true;
        }
        connection_status(
            ui,
            state,
            state.voiceger_connected,
            state.voiceger_launching,
        );
    });
    ui.horizontal(|ui| {
        ui.label("URL:");
        ui.text_edit_singleline(&mut state.config.voiceger_url);
    });
    ui.horizontal(|ui| {
        ui.label("起動コマンド:");
        ui.add(
            egui::TextEdit::singleline(&mut state.config.voiceger_path)
                .hint_text("空欄 = ~/voiceger_v2 のデフォルトコマンドを使用"),
        );
        if ui.button("参照").clicked() {
            if let Some(path) = rfd::FileDialog::new().pick_file() {
                state.config.voiceger_path = path.to_string_lossy().to_string();
                let _ = state.config.save();
            }
        }
    });
    hint(
        ui,
        "起動できない場合はサーバーのログ /tmp/zundux_voiceger.log を確認してください",
    );
    let theme = state.config.theme.clone();
    if widgets::toggle(
        ui,
        &theme,
        &mut state.config.auto_launch_voiceger,
        "アプリ起動時にVoicegerを自動起動",
    )
    .changed()
    {
        let _ = state.config.save();
    }

    sub_heading(ui, "参照音声");
    ui.horizontal(|ui| {
        ui.label("参照音声:");
        ui.add(
            egui::TextEdit::singleline(&mut state.config.voiceger_ref_audio)
                .hint_text("参照音声のパス"),
        );
        if ui.button("参照").clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("音声ファイル", &["wav", "flac", "mp3", "ogg", "m4a", "aac"])
                .pick_file()
            {
                state.config.voiceger_ref_audio = path.to_string_lossy().to_string();
                let _ = state.config.save();
            }
        }
    });
    hint(
        ui,
        "⚠ WAV・FLACを推奨。MP3・AACは非可逆圧縮のため高周波が欠落し、声質の再現精度が下がる場合があります。",
    );
    ui.horizontal(|ui| {
        ui.label("参照言語:");
        egui::ComboBox::from_id_salt("voiceger_prompt_lang")
            .selected_text(&state.config.voiceger_prompt_lang)
            .show_ui(ui, |ui| {
                for (code, name, _) in VOICEGER_LANGUAGES {
                    ui.selectable_value(
                        &mut state.config.voiceger_prompt_lang,
                        code.to_string(),
                        *name,
                    );
                }
            });
    });
    ui.horizontal(|ui| {
        ui.label("参照テキスト:");
        let resp = ui.add(
            egui::TextEdit::singleline(&mut state.config.voiceger_prompt_text)
                .hint_text("参照音声が喋っている内容"),
        );
        if resp.lost_focus() {
            let _ = state.config.save();
        }
    });
    widgets::toggle(
        ui,
        &theme,
        &mut state.config.voiceger_ref_free,
        "参照なしモード (ref_free) を常に使う",
    );
    hint(
        ui,
        "ON: 参照音声/参照テキストを使わず合成します。OFFでも短い英字（例: wa）や短い日本語（例: いいかな？）は自動で参照なしになります。",
    );
    ui.add_space(4.0);
    if ui.button("参照音声設定をデフォルトに戻す").clicked() {
        state.config.reset_voiceger_defaults();
        let _ = state.config.save();
    }
    hint(
        ui,
        "参照音声は {voicegerのフォルダ}/reference/ 以下にあります",
    );

    sub_heading(ui, "プリセット");
    show_preset_section(ui, state, &TtsEngineType::Voiceger, None);

    ui.add_space(4.0);
    ui.collapsing("Voiceger辞書", |ui| show_voiceger_dict(ui, state));
}

/// Per-language pronunciation replacements + shared silent words.
fn show_voiceger_dict(ui: &mut egui::Ui, state: &mut AppState) {
    hint(ui, "送信前にテキストを置換します（言語ごとに独立）");
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        for (code, name, _) in VOICEGER_LANGUAGES {
            let selected = state.voiceger_dict_lang == *code;
            if ui.selectable_label(selected, *name).clicked() {
                state.voiceger_dict_lang = code.to_string();
            }
        }
    });
    ui.separator();

    let lang = state.voiceger_dict_lang.clone();

    let mut to_delete_key: Option<String> = None;
    if let Some(lang_dict) = state.config.voiceger_dict.get(&lang) {
        let mut sorted: Vec<(String, String)> = lang_dict
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        for (surface, reading) in &sorted {
            ui.horizontal(|ui| {
                ui.label(format!("{} → {}", surface, reading));
                if ui.small_button("削除").clicked() {
                    to_delete_key = Some(surface.clone());
                }
            });
        }
    }
    if let Some(key) = to_delete_key {
        if let Some(d) = state.config.voiceger_dict.get_mut(&lang) {
            d.remove(&key);
        }
        let _ = state.config.save();
    }

    if !state.config.silent_words.is_empty() {
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("無音化（両エンジン共通）")
                .size(9.0)
                .color(state.config.theme.color(state.config.theme.text_muted)),
        );
        let mut silent_to_delete = None;
        for (i, word) in state.config.silent_words.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("{} → (無音)", word));
                if ui.small_button("削除").clicked() {
                    silent_to_delete = Some(i);
                }
            });
        }
        if let Some(idx) = silent_to_delete {
            state.config.silent_words.remove(idx);
            let _ = state.config.save();
        }
    }

    ui.add_space(4.0);
    ui.separator();

    ui.horizontal(|ui| {
        ui.label("表記:");
        ui.add(egui::TextEdit::singleline(&mut state.new_dict_surface).desired_width(90.0));
        ui.label("読み:");
        ui.add(
            egui::TextEdit::singleline(&mut state.new_dict_pronunciation)
                .desired_width(90.0)
                .hint_text("{silent}で無音化"),
        );
        if ui.button("追加").clicked()
            && !state.new_dict_surface.trim().is_empty()
            && !state.new_dict_pronunciation.trim().is_empty()
        {
            let surface = state.new_dict_surface.trim().to_string();
            let reading = state.new_dict_pronunciation.trim().to_string();
            if reading == "{silent}" {
                if !state.config.silent_words.contains(&surface) {
                    state.config.silent_words.push(surface);
                }
            } else {
                state
                    .config
                    .voiceger_dict
                    .entry(lang.clone())
                    .or_default()
                    .insert(surface, reading);
            }
            let _ = state.config.save();
            state.new_dict_surface.clear();
            state.new_dict_pronunciation.clear();
        }
    });
}

/// VOICEVOX-compatible engines the user added themselves.
fn show_other_engines_section(ui: &mut egui::Ui, state: &mut AppState) {
    hint(
        ui,
        "VOICEVOX互換API（/speakers, /audio_query, /synthesis）を持つエンジンを追加できます。",
    );
    ui.add_space(4.0);

    let mut to_delete: Option<usize> = None;
    let mut to_use: Option<usize> = None;
    for i in 0..state.config.generic_engines.len() {
        let is_active = state.config.active_engine == TtsEngineType::Generic
            && state.config.active_generic_engine_idx == i;
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label("名前:");
                ui.add(
                    egui::TextEdit::singleline(&mut state.config.generic_engines[i].name)
                        .desired_width(140.0)
                        .hint_text("エンジン名")
                        .id_salt(("gen_mgmt_name", i)),
                );
                if is_active {
                    ui.colored_label(
                        state.config.theme.color(state.config.theme.status_ok),
                        "✔ 使用中",
                    );
                } else if ui.small_button("このエンジンを使う").clicked() {
                    to_use = Some(i);
                }
            });
            ui.horizontal(|ui| {
                ui.label("URL:");
                ui.add(
                    egui::TextEdit::singleline(&mut state.config.generic_engines[i].url)
                        .desired_width(ui.available_width())
                        .hint_text("http://localhost:10101")
                        .id_salt(("gen_mgmt_url", i)),
                );
            });
            ui.horizontal(|ui| {
                if is_active {
                    if ui.button("接続確認").clicked() {
                        state.generic_connected = false;
                        state.pending_health_check_generic = true;
                    }
                    connection_status(ui, state, state.generic_connected, false);
                }
                if ui
                    .add(egui::Button::new(egui::RichText::new("削除").color(
                        state.config.theme.color(state.config.theme.status_error),
                    )))
                    .clicked()
                {
                    to_delete = Some(i);
                }
            });
            let engine_name = state.config.generic_engines[i].name.clone();
            ui.collapsing(format!("{} のプリセット", engine_name), |ui| {
                // Presets are keyed by engine; they are only usable while it is active.
                if !is_active {
                    hint(ui, "このエンジンを使うとプリセットを選択できます");
                }
                show_preset_section(ui, state, &TtsEngineType::Generic, Some(&engine_name));
            });
        });
        ui.add_space(4.0);
    }

    if let Some(i) = to_use {
        state.config.active_generic_engine_idx = i;
        state.generic_connected = false;
        state.pending_switch_engine = Some(TtsEngineType::Generic);
    }

    if let Some(i) = to_delete {
        let was_active = state.config.active_engine == TtsEngineType::Generic
            && state.config.active_generic_engine_idx == i;
        state.config.generic_engines.remove(i);
        if state.config.active_generic_engine_idx > i
            || state.config.active_generic_engine_idx >= state.config.generic_engines.len()
        {
            state.config.active_generic_engine_idx =
                state.config.active_generic_engine_idx.saturating_sub(1);
        }
        if was_active {
            // The engine in use is gone — fall back to VOICEVOX.
            state.pending_switch_engine = Some(TtsEngineType::Voicevox);
        }
        let _ = state.config.save();
    }

    let num_engines = state.config.generic_engines.len();
    if num_engines < 20 && ui.button("＋ エンジンを追加").clicked() {
        state.config.generic_engines.push(GenericEngineConfig {
            name: format!("エンジン{}", num_engines + 1),
            url: "http://localhost:10101".to_string(),
        });
        let _ = state.config.save();
    }
}

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    let theme = state.config.theme.clone();
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.heading("設定");
        ui.separator();

        ui.add_space(4.0);
        show_engine_selector(ui, state);
        ui.separator();
        ui.add_space(8.0);

        // ── Engines: one flat section each ─────────────────────────────────
        ui.collapsing("VOICEVOX", |ui| show_voicevox_section(ui, state));
        ui.add_space(8.0);
        ui.collapsing("Voiceger", |ui| show_voiceger_section(ui, state));
        ui.add_space(8.0);
        ui.collapsing("その他のエンジン", |ui| show_other_engines_section(ui, state));

        ui.add_space(8.0);

        // ── General ──────────────────────────────────────────────────────────
        ui.collapsing("General", |ui| {

        ui.collapsing("起動設定", |ui| {
            let mut autostart_app = AppConfig::is_autostart_enabled();
            if widgets::toggle(ui, &theme, &mut autostart_app, "システム起動時にアプリを自動起動")
                .changed()
            {
                state.config.auto_start_app = autostart_app;
                if let Err(e) = AppConfig::set_autostart(autostart_app) {
                    state.last_error = Some(format!("自動起動設定失敗: {}", e));
                }
            }
            hint(ui, "  ~/.config/autostart/ にデスクトップエントリを配置します");
            hint(ui, "  エンジンの自動起動は VOICEVOX / Voiceger の各セクションで設定できます");
        });

        ui.add_space(8.0);

        ui.collapsing("OSC設定", |ui| {
            widgets::toggle(ui, &theme, &mut state.config.osc_enabled, "OSCチャットボックス");
            ui.label("VRChatのチャットボックスにテキストを表示します");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("送信先アドレス:");
                ui.text_edit_singleline(&mut state.config.osc_address);
            });
            ui.horizontal(|ui| {
                ui.label("ポート:");
                let mut port_str = state.config.osc_port.to_string();
                if ui.text_edit_singleline(&mut port_str).changed() {
                    if let Ok(p) = port_str.parse::<u16>() {
                        state.config.osc_port = p;
                    }
                }
            });
        });

        ui.add_space(8.0);

        ui.collapsing("テンプレート", |ui| {
            widgets::toggle(
                ui,
                &theme,
                &mut state.config.templates_default_expanded,
                "起動時にテンプレートを展開した状態にする",
            );
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);
            let mut to_remove = None;
            for (i, template) in state.config.templates.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(template);
                    if ui.button("削除").clicked() { to_remove = Some(i); }
                });
            }
            if let Some(idx) = to_remove { state.config.templates.remove(idx); }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut state.new_template_text)
                    .hint_text("新しいテンプレート...").desired_width(200.0));
                if ui.button("追加").clicked() && !state.new_template_text.trim().is_empty() {
                    state.config.templates.push(state.new_template_text.trim().to_string());
                    state.new_template_text.clear();
                }
            });
        });

        ui.add_space(8.0);

        ui.collapsing("外観", |ui| {
            use crate::ui::theme::Theme;
            show_color_with_opacity(ui, state, "ウィンドウ背景", "window_background", |t| &mut t.window_background);
            show_color_with_opacity(ui, state, "タイトルバー背景", "titlebar_background", |t| &mut t.titlebar_background);
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            ui.label(egui::RichText::new("カラー設定（#RRGGBB または #RRGGBBAA）").small());
            ui.add_space(4.0);
            type ColorAccessor = fn(&mut crate::ui::theme::Theme) -> &mut [u8; 4];
            let color_fields: &[(&str, &str, ColorAccessor)] = &[
                ("タイトルバー文字", "titlebar_text", |t| &mut t.titlebar_text),
                ("パネル背景", "panel_background", |t| &mut t.panel_background),
                ("メイン文字", "text_primary", |t| &mut t.text_primary),
                ("サブ文字", "text_secondary", |t| &mut t.text_secondary),
                ("控えめ文字", "text_muted", |t| &mut t.text_muted),
                ("アクセント", "accent", |t| &mut t.accent),
                ("アクセント(ホバー)", "accent_hover", |t| &mut t.accent_hover),
                ("ボタン背景", "button_background", |t| &mut t.button_background),
                ("入力欄背景", "input_background", |t| &mut t.input_background),
                ("チップ背景", "chip_background", |t| &mut t.chip_background),
                ("タブ背景(選択)", "tab_active_background", |t| &mut t.tab_active_background),
                ("サイズ変更ハンドル背景", "resize_handle_background", |t| &mut t.resize_handle_background),
            ];
            for &(label, key, accessor) in color_fields {
                let current = *accessor(&mut state.config.theme);
                let buf = state.color_edit_buffers.entry(key.to_string()).or_insert_with(|| Theme::to_hex(current));
                ui.horizontal(|ui| {
                    let preview_color = state.config.theme.color(current);
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 2.0, preview_color);
                    ui.label(format!("{}:", label));
                    let response = ui.add(egui::TextEdit::singleline(buf).desired_width(100.0).hint_text("#RRGGBBAA"));
                    if response.changed() {
                        if let Some(parsed) = Theme::parse_hex(buf) {
                            *accessor(&mut state.config.theme) = parsed;
                            state.needs_theme_update = true;
                        }
                    }
                });
            }
            ui.add_space(4.0);
            if ui.button("デフォルトに戻す").clicked() {
                state.config.theme = Theme::default();
                state.color_edit_buffers.clear();
                state.needs_theme_update = true;
            }
        });

        }); // General

        ui.add_space(8.0);

        // ── Audio ─────────────────────────────────────────────────────────────
        ui.collapsing("Audio", |ui| {

        // Audio monitoring
        ui.collapsing("オーディオ", |ui| {
            widgets::toggle(
                ui,
                &theme,
                &mut state.config.monitor_audio,
                "自分にも音声を再生（モニター）",
            );
            hint(ui, "ON: 仮想マイクに加えてスピーカーからも音声が聞こえます");

            ui.add_space(4.0);
            if widgets::toggle(
                ui,
                &theme,
                &mut state.config.use_as_default_input,
                "ZunduxマイクをPCの既定の入力デバイスにする",
            )
            .changed()
            {
                let _ = state.config.save();
            }
            hint(
                ui,
                "ON: 入力デバイスを選べないアプリでもZunduxの音声がマイクとして使われます。OFFにするかアプリ終了時に元の入力へ戻します。",
            );

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("マイク入力:");
                if ui.button("更新").clicked() {
                    state.pending_refresh_mic_sources = true;
                }
            });

            if state.available_mic_sources.is_empty() {
                ui.label(
                    egui::RichText::new("マイクが見つかりません（更新を押してください）")
                        .small()
                        .weak(),
                );
            } else {
                let current = state.config.mic_source_name.clone().unwrap_or_default();
                let current_desc = state
                    .available_mic_sources
                    .iter()
                    .find(|(name, _)| *name == current)
                    .map(|(_, desc)| desc.as_str())
                    .unwrap_or("未選択");

                egui::ComboBox::from_id_salt("mic_source_combo")
                    .selected_text(current_desc)
                    .width(280.0)
                    .show_ui(ui, |ui| {
                        for (name, desc) in &state.available_mic_sources {
                            let is_selected =
                                state.config.mic_source_name.as_deref() == Some(name.as_str());
                            if ui.selectable_label(is_selected, desc).clicked() {
                                state.config.mic_source_name = Some(name.clone());
                                // Reconnect loopback with new source if mic is on
                                if state.mic_passthrough {
                                    state.pending_reconnect_mic = true;
                                }
                            }
                        }
                    });
            }
            ui.label(
                egui::RichText::new("MIC: ONで使用するマイクデバイスを選択します")
                    .small()
                    .weak(),
            );

            ui.add_space(4.0);
            let rnnoise_available =
                crate::audio::virtual_device::VirtualDevice::is_rnnoise_available();
            let old_ns = state.config.noise_suppression;
            widgets::toggle_enabled(
                ui,
                &theme,
                rnnoise_available,
                &mut state.config.noise_suppression,
                "ノイズキャンセル (RNNoise)",
            );
            if !rnnoise_available {
                ui.label(
                    egui::RichText::new(
                        "noise-suppression-for-voice がインストールされていません",
                    )
                    .small()
                    .weak()
                    .color(egui::Color32::from_rgb(255, 160, 0)),
                );
            } else {
                ui.label(
                    egui::RichText::new("AIベースのノイズ除去をマイク入力に適用します")
                        .small()
                        .weak(),
                );
            }
            if state.config.noise_suppression != old_ns && state.mic_passthrough {
                state.pending_reconnect_mic = true;
            }
        });

        ui.add_space(8.0);

        ui.collapsing("音声エフェクト", |ui| {
            if widgets::toggle(ui, &theme, &mut state.config.echo_enabled, "エコー").changed() {
                let _ = state.config.save();
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("遅延(ms):");
                ui.add(egui::Slider::new(&mut state.config.echo_delay_ms, 50..=500).step_by(10.0));
            });
            ui.horizontal(|ui| {
                ui.label("減衰:");
                ui.add(egui::Slider::new(&mut state.config.echo_decay, 0.1..=0.8).step_by(0.05));
            });
            ui.add_space(4.0);
            if widgets::toggle(
                ui,
                &theme,
                &mut state.config.effects_tts_only,
                "音声効果を合成音声のみに適用",
            )
            .changed()
            {
                let _ = state.config.save();
            }
            hint(
                ui,
                if state.config.effects_tts_only {
                    "ON: エコーは合成音声だけに掛かり、マイク入力はそのまま流れます"
                } else {
                    "OFF: エコーは合成音声とマイク入力（MIC: ON 時）の両方に掛かります"
                },
            );
        });

        ui.add_space(8.0);

        // Virtual device
        ui.collapsing("仮想デバイス", |ui| {
            ui.horizontal(|ui| {
                ui.label("デバイス名:");
                ui.text_edit_singleline(&mut state.config.virtual_device_name);
            });
            if !validation::is_valid_device_name(&state.config.virtual_device_name) {
                ui.colored_label(
                    state.config.theme.color(state.config.theme.status_warn),
                    "デバイス名は英数字、_、- のみ (最大64文字)",
                );
            }
            ui.label(
                egui::RichText::new(
                    "名前変更は「削除」→「作成」で反映されます（アプリの再起動は不要）",
                )
                .small()
                .weak(),
            );
            ui.horizontal(|ui| {
                if ui.button("作成").clicked() {
                    state.pending_create_device = true;
                }
                if ui.button("削除").clicked() {
                    state.pending_destroy_device = true;
                }
            });
            if state.device_ready {
                ui.colored_label(
                    state.config.theme.color(state.config.theme.status_ok),
                    format!("マイクソース: {}.monitor", state.config.virtual_device_name),
                );
            }
        });

        ui.add_space(8.0);

        ui.collapsing("サウンドボード", |ui| {
            ui.horizontal(|ui| {
                ui.label("フォルダパス:");
                if ui.button("参照").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        state.config.soundboard_path = path.to_string_lossy().to_string();
                        state.pending_soundboard_scan = true;
                    }
                }
                ui.add(egui::TextEdit::singleline(&mut state.config.soundboard_path)
                    .desired_width(f32::INFINITY));
            });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("ターゲット音量 (LUFS):");
                ui.add(egui::Slider::new(&mut state.config.target_lufs, -96.0..=-6.0).step_by(0.5));
            });

            ui.horizontal(|ui| {
                ui.label("許容範囲 (±LUFS):");
                ui.add(
                    egui::Slider::new(&mut state.config.loudness_tolerance, 1.0..=6.0).step_by(0.5),
                );
            });

            ui.add_space(4.0);
            if !state.config.soundboard_gains.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "調整済み: {}ファイル",
                            state.config.soundboard_gains.len()
                        ))
                        .size(10.0)
                        .color(state.config.theme.color(state.config.theme.text_muted)),
                    );
                    if ui.button("ゲインリセット").clicked() {
                        state.config.soundboard_gains.clear();
                        let _ = state.config.save();
                        state.pending_loudness_scan = true;
                    }
                });
            }
        });

        }); // Audio

        ui.add_space(12.0);

        // Warn when no presets exist
        let voicevox_presets = state.config.presets.iter().any(|p| p.engine == TtsEngineType::Voicevox);
        let voiceger_presets = state.config.presets.iter().any(|p| p.engine == TtsEngineType::Voiceger);
        if !voicevox_presets || !voiceger_presets {
            let warn_color = state.config.theme.color(state.config.theme.status_warn);
            if !voicevox_presets {
                ui.colored_label(warn_color, "⚠ VOICEVOXプリセットがありません。設定で追加してください。");
            }
            if !voiceger_presets {
                ui.colored_label(warn_color, "⚠ Voicegerプリセットがありません。設定で追加してください。");
            }
        }

        if ui.button("設定を保存").clicked() {
            match state.config.save() {
                Ok(()) => state.last_error = None,
                Err(e) => state.last_error = Some(format!("保存失敗: {}", e)),
            }
        }

        ui.add_space(24.0);
        ui.separator();
        ui.add_space(8.0);

        // Credits
        ui.collapsing("Credits", |ui| {
            let muted = state.config.theme.color(state.config.theme.text_muted);
            let secondary = state.config.theme.color(state.config.theme.text_secondary);

            ui.label(egui::RichText::new("ZunduxTTS").size(13.0).color(secondary));
            ui.label(
                egui::RichText::new("A Linux desktop TTS virtual microphone for VRChat.")
                    .small()
                    .color(muted),
            );
            ui.add_space(8.0);

            let credits = [
                ("VOICEVOX", "High-quality Japanese TTS engine", "https://voicevox.hiroshiba.jp/"),
                ("Voiceger / GPT-SoVITS", "Multilingual voice synthesis", "https://github.com/zunzun999/voiceger_v2"),
                ("egui / eframe", "Immediate-mode GUI framework", "https://github.com/emilk/egui"),
                ("ZUNDAMON character", "© AHS Co., Ltd.", "https://zunko.jp/"),
            ];

            for (name, desc, _url) in &credits {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(*name).small().color(secondary));
                    ui.label(egui::RichText::new(format!("— {}", desc)).small().color(muted));
                });
            }

            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("Built with Rust  •  Licensed under MIT")
                    .small()
                    .color(muted),
            );
        });

        ui.add_space(8.0);
    });
}

/// Color hex input (RGB only) + transparency slider for a single RGBA field.
fn show_color_with_opacity(
    ui: &mut egui::Ui,
    state: &mut AppState,
    label: &str,
    key: &str,
    accessor: fn(&mut crate::ui::theme::Theme) -> &mut [u8; 4],
) {
    use crate::ui::theme::Theme;

    let current = *accessor(&mut state.config.theme);
    let rgb_hex = format!("#{:02X}{:02X}{:02X}", current[0], current[1], current[2]);
    let buf = state
        .color_edit_buffers
        .entry(key.to_string())
        .or_insert_with(|| rgb_hex.clone());

    ui.horizontal(|ui| {
        // Color preview swatch
        let preview_color = state.config.theme.color(current);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 2.0, preview_color);

        ui.label(format!("{}:", label));
        let response = ui.add(
            egui::TextEdit::singleline(buf)
                .desired_width(80.0)
                .hint_text("#RRGGBB"),
        );
        if response.changed() {
            if let Some(parsed) = Theme::parse_hex(buf) {
                let field = accessor(&mut state.config.theme);
                // Only update RGB, preserve current alpha
                field[0] = parsed[0];
                field[1] = parsed[1];
                field[2] = parsed[2];
                state.needs_theme_update = true;
            }
        }

        // White (#FFFFFF) uses premultiplied alpha — lowering alpha has no visible effect
        let rgb = accessor(&mut state.config.theme);
        let is_white = rgb[0] == 255 && rgb[1] == 255 && rgb[2] == 255;

        let mut opacity = accessor(&mut state.config.theme)[3] as f32 / 255.0 * 100.0;
        if ui
            .add_enabled(
                !is_white,
                egui::Slider::new(&mut opacity, 0.0..=100.0).suffix("%"),
            )
            .changed()
        {
            let alpha = (opacity / 100.0 * 255.0).round() as u8;
            accessor(&mut state.config.theme)[3] = alpha;
            state.needs_theme_update = true;
        }
    });
    if *accessor(&mut state.config.theme) == [255, 255, 255, current[3]]
        && current[0] == 255
        && current[1] == 255
        && current[2] == 255
    {
        ui.label(
            egui::RichText::new("  白(#FFFFFF)はpremultiplied alphaのため透明度を変更できません")
                .small()
                .weak(),
        );
    }
}
