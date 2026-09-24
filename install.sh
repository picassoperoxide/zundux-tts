#!/usr/bin/env bash
set -euo pipefail

# ===== ZunduxTTS インストーラー (Arch/Manjaro) =====

BOLD='\033[1m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
RED='\033[0;31m'
NC='\033[0m'

info()  { echo -e "${GREEN}[INFO]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*"; }

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
INSTALL_BIN="$HOME/.local/bin"
INSTALL_APPS="$HOME/.local/share/applications"
INSTALL_ICONS="$HOME/.local/share/icons/hicolor/256x256/apps"
CONFIG_DIR="$HOME/.config/zundux_tts"
GITHUB_REPO="Pancreas-yk/zundux-tts"

escape_ere() {
    printf '%s' "$1" | sed -e 's/[][(){}.^$*+?|\\]/\\&/g'
}

# config.toml のトップレベルにキーを設定する（既存なら置換、無ければ最初のテーブルの前に挿入）。
# 末尾に追記すると最後の [table] の中に入ってしまいアプリから読まれないため、必ずトップレベルに書く。
# $3 は TOML の値としてそのまま書かれる（文字列なら呼び出し側で "..." を付ける）。
set_toplevel_key() {
    local file="$1" tmp
    tmp="$(mktemp)"
    KEY="$2" LINE="$2 = $3" awk '
        BEGIN { done = 0; top = 1; key = ENVIRON["KEY"]; line = ENVIRON["LINE"] }
        top && /^[[:space:]]*\[/ { if (!done) { print line; done = 1 } top = 0 }
        top {
            s = $0; sub(/^[[:space:]]+/, "", s)
            if (index(s, key) == 1 && substr(s, length(key) + 1) ~ /^[[:space:]]*=/) {
                if (!done) { print line; done = 1 }
                next
            }
        }
        { print }
        END { if (!done) print line }
    ' "$file" > "$tmp" && mv "$tmp" "$file"
}

download_binary() {
    info "最新リリースをダウンロード中..."

    local api_url="https://api.github.com/repos/${GITHUB_REPO}/releases/latest"
    local release_json
    release_json=$(curl -fsSL "$api_url") || {
        error "リリース情報の取得に失敗しました"
        return 1
    }

    local binary_url
    binary_url=$(echo "$release_json" | grep -o '"browser_download_url":[[:space:]]*"[^"]*zundux_tts-linux-x86_64"' | grep -o 'https://[^"]*')

    local release_repo
    release_repo=$(echo "$release_json" | sed -nE 's/.*"assets_url":[[:space:]]*"https:\/\/api\.github\.com\/repos\/([^"]+)\/releases\/[0-9]+\/assets".*/\1/p' | head -n 1)

    local allowed_repo_pattern
    allowed_repo_pattern="$(escape_ere "$GITHUB_REPO")"
    if [ -n "$release_repo" ] && [ "$release_repo" != "$GITHUB_REPO" ]; then
        allowed_repo_pattern="${allowed_repo_pattern}|$(escape_ere "$release_repo")"
    fi

    # Validate URL pattern
    if ! echo "$binary_url" | grep -qE "^https://github\.com/(${allowed_repo_pattern})/releases/download/v[0-9]+\.[0-9]+\.[0-9]+/zundux_tts-linux-x86_64$"; then
        error "ダウンロードURLが不正です: $binary_url"
        return 1
    fi

    local checksum_url
    checksum_url=$(echo "$release_json" | grep -o '"browser_download_url":[[:space:]]*"[^"]*SHA256SUMS"' | grep -o 'https://[^"]*')
    if ! echo "$checksum_url" | grep -qE "^https://github\.com/(${allowed_repo_pattern})/releases/download/v[0-9]+\.[0-9]+\.[0-9]+/SHA256SUMS$"; then
        error "チェックサムURLが不正です: $checksum_url"
        return 1
    fi

    local tmpdir
    tmpdir=$(mktemp -d)
    trap "rm -rf '$tmpdir'" EXIT

    curl -fsSL "$binary_url" -o "$tmpdir/zundux_tts-linux-x86_64" || {
        error "バイナリのダウンロードに失敗しました"
        return 1
    }

    curl -fsSL "$checksum_url" -o "$tmpdir/SHA256SUMS" || {
        error "チェックサムのダウンロードに失敗しました"
        return 1
    }

    cd "$tmpdir"
    if ! sha256sum -c SHA256SUMS; then
        error "チェックサム検証に失敗しました。ダウンロードが破損している可能性があります"
        return 1
    fi
    cd "$SCRIPT_DIR"

    cp "$tmpdir/zundux_tts-linux-x86_64" "$INSTALL_BIN/zundux_tts"
    chmod +x "$INSTALL_BIN/zundux_tts"
    info "バイナリをインストール: $INSTALL_BIN/zundux_tts"
}

# ---------- アンインストール ----------
if [ "${1:-}" = "--uninstall" ]; then
    echo -e "${BOLD}ZunduxTTS をアンインストールします${NC}"
    echo ""

    # Stop and remove Docker container
    if docker ps --format '{{.Names}}' 2>/dev/null | grep -q "^zundux-voicevox$"; then
        info "VOICEVOXコンテナを停止中..."
        docker stop zundux-voicevox 2>/dev/null || true
    fi

    # Remove installed files
    for f in "$INSTALL_BIN/zundux_tts" "$INSTALL_BIN/zundux_tts_launch.sh" \
             "$INSTALL_APPS/zundux_tts.desktop" "$INSTALL_ICONS/zundux_tts.png"; do
        if [ -f "$f" ]; then
            rm "$f"
            info "削除: $f"
        fi
    done

    # Update icon cache
    if command -v gtk-update-icon-cache &>/dev/null; then
        gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
    fi

    # Ask about config
    if [ -d "$CONFIG_DIR" ]; then
        echo ""
        read -rp "設定ファイルも削除しますか？ ($CONFIG_DIR) [y/N]: " del_config
        if [ "$del_config" = "y" ] || [ "$del_config" = "Y" ]; then
            rm -rf "$CONFIG_DIR"
            info "設定ディレクトリを削除: $CONFIG_DIR"
        else
            info "設定ファイルを保持しました"
        fi
    fi

    # Ask about Docker image
    echo ""
    read -rp "VOICEVOXのDockerイメージも削除しますか？ [y/N]: " del_image
    if [ "$del_image" = "y" ] || [ "$del_image" = "Y" ]; then
        docker images --format '{{.Repository}}:{{.Tag}}' 2>/dev/null | grep "^voicevox/voicevox_engine" | while read -r img; do
            docker rmi "$img" 2>/dev/null && info "Dockerイメージを削除: $img"
        done
    fi

    # Ask about Voiceger
    echo ""
    read -rp "Voicegerのフォルダとconda環境も削除しますか？ [y/N]: " del_voiceger
    if [ "$del_voiceger" = "y" ] || [ "$del_voiceger" = "Y" ]; then
        read -rp "Voicegerのフォルダのパス [デフォルト: $HOME/voiceger_v2]: " vgr_dir
        vgr_dir="${vgr_dir:-$HOME/voiceger_v2}"
        if [ -d "$vgr_dir" ]; then
            rm -rf "$vgr_dir"
            info "削除: $vgr_dir"
        fi
        # Remove conda env
        for conda_cmd in conda "$HOME/miniconda3/bin/conda" "$HOME/anaconda3/bin/conda"; do
            if command -v "$conda_cmd" &>/dev/null 2>&1; then
                "$conda_cmd" env remove -n voiceger -y 2>/dev/null && info "conda環境 'voiceger' を削除しました"
                break
            fi
        done
    fi

    echo ""
    info "アンインストールが完了しました"
    exit 0
fi

# ---------- Step 1: 依存パッケージのインストール ----------
info "依存パッケージを確認中..."

if [ "${1:-}" = "--from-source" ]; then
    PACKAGES=(base-devel rust docker noto-fonts-cjk ffmpeg)
else
    PACKAGES=(docker noto-fonts-cjk ffmpeg)
fi

# PulseAudio tools (pactl, paplay) can come from pulseaudio or pipewire-pulse
if ! command -v pactl &>/dev/null; then
    # Prefer pipewire-pulse on modern systems, fall back to pulseaudio
    if pacman -Si pipewire-pulse &>/dev/null; then
        PACKAGES+=(pipewire-pulse)
    else
        PACKAGES+=(pulseaudio)
    fi
fi

MISSING=()

for pkg in "${PACKAGES[@]}"; do
    if ! pacman -Qi "$pkg" &>/dev/null; then
        MISSING+=("$pkg")
    fi
done

if [ ${#MISSING[@]} -gt 0 ]; then
    info "以下のパッケージをインストールします: ${MISSING[*]}"
    sudo pacman -S --needed --noconfirm "${MISSING[@]}"
else
    info "すべての依存パッケージがインストール済みです"
fi

# Optional: RNNoise noise suppression
if ! pacman -Qi noise-suppression-for-voice &>/dev/null; then
    echo ""
    info "noise-suppression-for-voice (ノイズキャンセル) が未インストールです"
    read -rp "インストールしますか？ (AURヘルパーが必要) [y/N]: " install_rnnoise
    if [ "$install_rnnoise" = "y" ] || [ "$install_rnnoise" = "Y" ]; then
        if command -v yay &>/dev/null; then
            yay -S --needed noise-suppression-for-voice
        elif command -v paru &>/dev/null; then
            paru -S --needed noise-suppression-for-voice
        else
            warn "AURヘルパー (yay/paru) が見つかりません。手動でインストールしてください"
        fi
    fi
fi

# ---------- Step 2: GPU検出 & VOICEVOXイメージ選択 ----------
VOICEVOX_IMAGE="voicevox/voicevox_engine:latest"
GPU_FLAGS=""

if lspci | grep -qi nvidia; then
    echo ""
    echo -e "${BOLD}NVIDIA GPUが検出されました。GPU版VOICEVOXを使いますか？${NC}"
    echo "GPU版は合成速度が大幅に速くなります。"
    echo "  1) GPU版 (nvidia-container-toolkit が必要です)"
    echo "  2) CPU版"
    echo ""
    read -rp "選択 [1/2] (デフォルト: 1): " gpu_choice
    gpu_choice="${gpu_choice:-1}"

    if [ "$gpu_choice" = "1" ]; then
        VOICEVOX_IMAGE="voicevox/voicevox_engine:nvidia-latest"
        GPU_FLAGS="--gpus all"

        # nvidia-container-toolkit のインストール
        if ! pacman -Qi nvidia-container-toolkit &>/dev/null; then
            info "nvidia-container-toolkit をインストール中..."
            # Try pacman first, fall back to AUR
            if pacman -Si nvidia-container-toolkit &>/dev/null; then
                sudo pacman -S --needed --noconfirm nvidia-container-toolkit
            else
                warn "nvidia-container-toolkit が公式リポジトリにありません"
                warn "AUR からインストールしてください: yay -S nvidia-container-toolkit"
                echo ""
                read -rp "続行しますか？ (y/N): " cont
                if [ "$cont" != "y" ] && [ "$cont" != "Y" ]; then
                    error "インストールを中断しました"
                    exit 1
                fi
            fi
        fi
        info "GPU版VOICEVOXを使用します"
    else
        info "CPU版VOICEVOXを使用します"
    fi
else
    info "NVIDIA GPUが検出されませんでした。CPU版VOICEVOXを使用します"
fi

# ---------- Step 3: Docker設定 ----------
info "Dockerを設定中..."

# Enable and start docker service
if ! systemctl is-active --quiet docker; then
    sudo systemctl enable --now docker
    info "Dockerサービスを起動しました"
fi

# Add user to docker group
if ! groups "$USER" | grep -q '\bdocker\b'; then
    sudo usermod -aG docker "$USER"
    NEED_RELOGIN=true
    warn "ユーザーをdockerグループに追加しました（再ログインが必要です）"
else
    NEED_RELOGIN=false
fi

# Pull VOICEVOX image
info "VOICEVOXイメージをダウンロード中... (時間がかかる場合があります)"
if [ "$NEED_RELOGIN" = true ]; then
    sudo docker pull "$VOICEVOX_IMAGE"
else
    docker pull "$VOICEVOX_IMAGE"
fi

# ---------- Step 4: ビルド or ダウンロード ----------
mkdir -p "$INSTALL_BIN"
mkdir -p "$INSTALL_APPS"
mkdir -p "$CONFIG_DIR"
mkdir -p "$INSTALL_ICONS"

if [ "${1:-}" = "--from-source" ]; then
    info "ソースからビルド中..."
    if ! command -v cargo &>/dev/null; then
        error "Rustツールチェーンが必要です: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        exit 1
    fi
    cd "$SCRIPT_DIR"
    cargo build --release
    cp "$SCRIPT_DIR/target/release/zundux_tts" "$INSTALL_BIN/zundux_tts"
    info "バイナリをインストール: $INSTALL_BIN/zundux_tts"
else
    download_binary
fi

# Build docker run command
DOCKER_CMD="docker run --rm ${GPU_FLAGS:+$GPU_FLAGS }-p 50021:50021 $VOICEVOX_IMAGE"

# Create launch script
cat > "$INSTALL_BIN/zundux_tts_launch.sh" << 'LAUNCHER_EOF'
#!/usr/bin/env bash
VOICEVOX_CONTAINER="zundux-voicevox"

# Force X11 backend for IME (Japanese input) support on Wayland
# (WINIT_UNIX_BACKEND was removed in winit 0.29+; unsetting WAYLAND_DISPLAY forces X11)
unset WAYLAND_DISPLAY

# Set XMODIFIERS if not already set (detect running IME)
if [ -z "${XMODIFIERS:-}" ]; then
    if pgrep -x fcitx5 >/dev/null 2>&1 || pgrep -x fcitx >/dev/null 2>&1; then
        export XMODIFIERS=@im=fcitx
    elif pgrep -x ibus-daemon >/dev/null 2>&1; then
        export XMODIFIERS=@im=ibus
    fi
fi
LAUNCHER_EOF

# Append image and GPU flags safely
printf 'VOICEVOX_IMAGE=%q\n' "$VOICEVOX_IMAGE" >> "$INSTALL_BIN/zundux_tts_launch.sh"
printf 'GPU_FLAGS=%q\n' "$GPU_FLAGS" >> "$INSTALL_BIN/zundux_tts_launch.sh"

cat >> "$INSTALL_BIN/zundux_tts_launch.sh" << 'LAUNCHER_EOF'

# Start VOICEVOX if not running
if ! docker ps --format '{{.Names}}' | grep -q "^${VOICEVOX_CONTAINER}$"; then
    docker run -d --rm --name "$VOICEVOX_CONTAINER" ${GPU_FLAGS:+$GPU_FLAGS} -p 50021:50021 "$VOICEVOX_IMAGE" >/dev/null 2>&1
fi

# Run the app
"$HOME/.local/bin/zundux_tts"

# Stop VOICEVOX on exit
docker stop "$VOICEVOX_CONTAINER" 2>/dev/null || true
LAUNCHER_EOF

chmod +x "$INSTALL_BIN/zundux_tts_launch.sh"
info "ランチャーをインストール: $INSTALL_BIN/zundux_tts_launch.sh"

# Create .desktop file
cat > "$INSTALL_APPS/zundux_tts.desktop" << DESKTOP_EOF
[Desktop Entry]
Version=1.0
Type=Application
Name=ZunduxTTS
Comment=VOICEVOX TTS virtual microphone
Exec=$INSTALL_BIN/zundux_tts_launch.sh
Icon=zundux_tts
Terminal=false
StartupWMClass=zundux_tts
Categories=AudioVideo;Audio;
DESKTOP_EOF

info "デスクトップエントリをインストール: $INSTALL_APPS/zundux_tts.desktop"

# Install icon
if [ -f "$SCRIPT_DIR/assets/design-1.png" ]; then
    cp "$SCRIPT_DIR/assets/design-1.png" "$INSTALL_ICONS/zundux_tts.png"
else
    # Download icon from repository when running without source checkout
    curl -fsSL "https://raw.githubusercontent.com/${GITHUB_REPO}/master/assets/design-1.png" -o "$INSTALL_ICONS/zundux_tts.png" 2>/dev/null || \
        warn "アイコンのダウンロードに失敗しました（アプリの動作には影響しません）"
fi
info "アイコンをインストール: $INSTALL_ICONS/zundux_tts.png"

# Update icon cache
if command -v gtk-update-icon-cache &>/dev/null; then
    gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
fi

# Update config with VOICEVOX Docker settings
if [ -f "$CONFIG_DIR/config.toml" ]; then
    # Update existing config (top-level keys win over [engine_configs.*] mirrors)
    set_toplevel_key "$CONFIG_DIR/config.toml" "voicevox_path" "\"$DOCKER_CMD\""
    set_toplevel_key "$CONFIG_DIR/config.toml" "auto_launch_voicevox" "true"
else
    # Create minimal config
    cat > "$CONFIG_DIR/config.toml" << CONFIG_EOF
voicevox_path = "$DOCKER_CMD"
auto_launch_voicevox = true
CONFIG_EOF
fi

info "設定ファイルを更新: $CONFIG_DIR/config.toml"

# ---------- Step 6: Voicegerのインストール（任意） ----------
echo ""
echo -e "${BOLD}Voiceger（英語・多言語TTS）をインストールしますか？${NC}"
echo "  ずんだもんの声で日本語・英語・中国語・韓国語・広東語が喋れます"
echo "  GPT-SoVITSベース、GPU推奨、約5GBのストレージが必要です"
echo ""
read -rp "インストールしますか？ [y/N]: " install_voiceger
VOICEGER_INSTALLED=false

if [ "$install_voiceger" = "y" ] || [ "$install_voiceger" = "Y" ]; then

    # --- condaの確認・インストール ---
    CONDA_CMD=""
    for candidate in conda "$HOME/miniconda3/bin/conda" "$HOME/anaconda3/bin/conda" \
                     "/opt/miniconda3/bin/conda" "/opt/anaconda3/bin/conda"; do
        if command -v "$candidate" &>/dev/null 2>&1; then
            CONDA_CMD="$candidate"
            break
        fi
    done

    if [ -z "$CONDA_CMD" ]; then
        info "Minicondaをインストール中..."
        local_miniconda="$HOME/miniconda3"
        curl -fsSL "https://repo.anaconda.com/miniconda/Miniconda3-latest-Linux-x86_64.sh" \
            -o /tmp/miniconda_install.sh
        bash /tmp/miniconda_install.sh -b -p "$local_miniconda"
        rm -f /tmp/miniconda_install.sh
        CONDA_CMD="$local_miniconda/bin/conda"
        # condaをPATHに追加（現セッションのみ）
        export PATH="$local_miniconda/bin:$PATH"
        "$CONDA_CMD" init bash 2>/dev/null || true
        info "Minicondaをインストールしました: $local_miniconda"
    else
        info "conda が見つかりました: $CONDA_CMD"
    fi

    # --- インストール先の選択 ---
    echo ""
    read -rp "Voicegerのインストール先 [デフォルト: $HOME/voiceger_v2]: " voiceger_dir
    VOICEGER_DIR="${voiceger_dir:-$HOME/voiceger_v2}"

    # --- リポジトリのクローン ---
    if [ -d "$VOICEGER_DIR/.git" ]; then
        info "既存のVoicegerリポジトリを更新中: $VOICEGER_DIR"
        git -C "$VOICEGER_DIR" pull --ff-only 2>/dev/null || warn "git pull に失敗しました（ローカル変更がある可能性があります）"
    else
        info "Voicegerをクローン中: $VOICEGER_DIR"
        git clone --depth=1 "https://github.com/zunzun999/voiceger_v2.git" "$VOICEGER_DIR"
    fi

    # --- conda ToS承認（未承認だと env create が無言で失敗する） ---
    info "conda利用規約を確認中..."
    "$CONDA_CMD" tos accept --override-channels \
        --channel https://repo.anaconda.com/pkgs/main 2>/dev/null || true
    "$CONDA_CMD" tos accept --override-channels \
        --channel https://repo.anaconda.com/pkgs/r 2>/dev/null || true

    # --- conda環境の作成 ---
    if "$CONDA_CMD" env list 2>/dev/null | grep -q "^voiceger "; then
        info "conda環境 'voiceger' は既に存在します"
    else
        info "conda環境 'voiceger' を作成中 (Python 3.9)..."
        "$CONDA_CMD" create -n voiceger python=3.9 -y
        # 作成を確認
        if ! "$CONDA_CMD" env list 2>/dev/null | grep -q "^voiceger "; then
            error "conda環境の作成に失敗しました。condaのログを確認してください"
            exit 1
        fi
        info "conda環境 'voiceger' を作成しました"
    fi

    # condaのenvs dir から直接Pythonパスを取得（conda run より確実）
    VOICEGER_PYTHON="$("$CONDA_CMD" run -n voiceger python -c 'import sys; print(sys.executable)' 2>/dev/null)"
    if [ -z "$VOICEGER_PYTHON" ] || [ ! -f "$VOICEGER_PYTHON" ]; then
        # フォールバック: condaのenvs dirから推測
        CONDA_BASE="$(dirname "$(dirname "$CONDA_CMD")")"
        VOICEGER_PYTHON="$CONDA_BASE/envs/voiceger/bin/python"
    fi
    info "Python: $VOICEGER_PYTHON"

    # --- CUDAバージョンの検出（nvcc不要、nvidia-smiで判定）---
    CUDA_VER="cu118"
    if command -v nvidia-smi &>/dev/null; then
        driver_ver=$(nvidia-smi --query-gpu=driver_version --format=csv,noheader 2>/dev/null | head -1)
        driver_major=$(echo "$driver_ver" | cut -d. -f1)
        # ドライバー 525+ は CUDA 12.x をサポート
        if [ "${driver_major:-0}" -ge 525 ]; then
            CUDA_VER="cu121"
        fi
    elif command -v nvcc &>/dev/null; then
        nvcc_major=$(nvcc --version 2>/dev/null | grep -oP 'release \K[0-9]+' | head -1)
        if [ "${nvcc_major:-0}" -ge 12 ]; then
            CUDA_VER="cu121"
        fi
    fi
    info "CUDAバックエンド: $CUDA_VER"

    # --- PyTorchのインストール（既にインストール済みならスキップ） ---
    if "$VOICEGER_PYTHON" -c "import torch; print(torch.__version__)" 2>/dev/null | grep -q "^2\."; then
        info "PyTorch は既にインストール済みです ($(\"$VOICEGER_PYTHON\" -c 'import torch; print(torch.__version__)' 2>/dev/null))"
    else
        info "PyTorch (${CUDA_VER}) をインストール中... (約2GB、時間がかかります)"
        "$VOICEGER_PYTHON" -m pip install \
            torch==2.1.2 torchvision==0.16.2 torchaudio==2.1.2 \
            --index-url "https://download.pytorch.org/whl/${CUDA_VER}"
        info "PyTorchのインストールが完了しました"
    fi

    # --- Voiceger依存パッケージのインストール ---
    info "Voicegerの依存パッケージをインストール中..."
    # scipy: Python 3.9 では 1.11.x が上限のため先にピン
    if ! "$VOICEGER_PYTHON" -c "import scipy" 2>/dev/null; then
        "$VOICEGER_PYTHON" -m pip install "scipy==1.11.4" || warn "scipy のインストールに失敗しました"
    fi
    if [ -f "$VOICEGER_DIR/GPT-SoVITS/requirements.txt" ]; then
        "$VOICEGER_PYTHON" -m pip install \
            --ignore-installed scipy \
            -r "$VOICEGER_DIR/GPT-SoVITS/requirements.txt" \
            || warn "一部の依存パッケージのインストールに失敗しました（動作に影響がない場合があります）"
    fi
    # soxr: transformers が内部で要求するが requirements.txt に含まれていない
    "$VOICEGER_PYTHON" -m pip install soxr || warn "soxr のインストールに失敗しました"

    # --- LangSegment 0.2.0 の __init__.py パッチ ---
    LANGSEG_INIT=$("$VOICEGER_PYTHON" -c \
        "import importlib.util, os; s=importlib.util.find_spec('LangSegment'); print(os.path.join(os.path.dirname(s.origin),'__init__.py'))" 2>/dev/null)
    if [ -n "$LANGSEG_INIT" ] && grep -q "setLangfilters" "$LANGSEG_INIT" 2>/dev/null; then
        if ! grep -q "setLangfilters = setfilters" "$LANGSEG_INIT" 2>/dev/null; then
            info "LangSegment __init__.py にエイリアスを追加中..."
            sed -i 's/from \.LangSegment import \(.*\)setLangfilters,getLangfilters,\(.*\)/from .LangSegment import \1\2/' "$LANGSEG_INIT"
            printf '\n# aliases missing from this version\nsetLangfilters = setfilters\ngetLangfilters = getfilters\n' >> "$LANGSEG_INIT"
        fi
    fi

    # --- CUDA対応のffmpegとその他システム依存 ---
    VOICEGER_PKGS=(ffmpeg)
    VOICEGER_MISSING=()
    for pkg in "${VOICEGER_PKGS[@]}"; do
        if ! pacman -Qi "$pkg" &>/dev/null; then
            VOICEGER_MISSING+=("$pkg")
        fi
    done
    if [ ${#VOICEGER_MISSING[@]} -gt 0 ]; then
        sudo pacman -S --needed --noconfirm "${VOICEGER_MISSING[@]}"
    fi

    # --- G2PWModel（中国語TTS用ピンイン推論モデル）のダウンロード ---
    G2PW_DIR="$VOICEGER_DIR/GPT-SoVITS/GPT_SoVITS/text/G2PWModel"
    if [ ! -f "$G2PW_DIR/g2pW.onnx" ]; then
        info "G2PWModel（中国語ピンイン推論）をダウンロード中..."
        G2PW_DIR="$G2PW_DIR" "$VOICEGER_PYTHON" - <<'PYEOF'
from huggingface_hub import snapshot_download
import os
dest = os.environ["G2PW_DIR"]
snapshot_download(repo_id="alextomcat/G2PWModel", local_dir=dest)
print("G2PWModel ダウンロード完了")
PYEOF
        info "G2PWModel のダウンロードが完了しました"
    else
        info "G2PWModel は既に存在します"
    fi

    # --- GPT-SoVITS 事前学習モデルのダウンロード ---
    PRETRAINED_DIR="$VOICEGER_DIR/GPT-SoVITS/GPT_SoVITS/pretrained_models"
    GSV_CKPT="$PRETRAINED_DIR/gsv-v2final-pretrained/s1bert25hz-5kh-longer-epoch=12-step=369668.ckpt"
    if [ ! -f "$GSV_CKPT" ]; then
        info "GPT-SoVITS 事前学習モデルをダウンロード中... (数GB、時間がかかります)"
        PRETRAINED_DIR="$PRETRAINED_DIR" "$VOICEGER_PYTHON" - <<'PYEOF'
from huggingface_hub import snapshot_download
import os
dest = os.environ["PRETRAINED_DIR"]
snapshot_download(repo_id="lj1995/GPT-SoVITS", local_dir=dest,
                  ignore_patterns=["*.git*", ".gitattributes"])
print("GPT-SoVITS 事前学習モデルのダウンロード完了")
PYEOF
        info "GPT-SoVITS 事前学習モデルのダウンロードが完了しました"
    else
        info "GPT-SoVITS 事前学習モデルは既に存在します"
    fi

    # --- ずんだもん Fine-tuned モデルのダウンロード ---
    ZUNDAMON_MODEL_DIR="$VOICEGER_DIR/GPT-SoVITS/zundamon_models"
    if [ ! -d "$ZUNDAMON_MODEL_DIR" ] || [ -z "$(ls -A "$ZUNDAMON_MODEL_DIR" 2>/dev/null)" ]; then
        info "ずんだもん Fine-tuned モデルをダウンロード中..."
        "$VOICEGER_PYTHON" -c "
from huggingface_hub import snapshot_download
import os
dest = os.path.expanduser('$ZUNDAMON_MODEL_DIR')
snapshot_download(repo_id='zunzunpj/zundamon_GPT-SoVITS', local_dir=dest,
                  ignore_patterns=['*.git*'])
print('ずんだもんモデルのダウンロード完了')
"
        info "ずんだもん Fine-tuned モデルのダウンロードが完了しました"
    else
        info "ずんだもん Fine-tuned モデルは既に存在します"
    fi

    # --- tts_infer.yaml に GPU設定を適用（サーバー起動前に設定しないと上書きされる） ---
    TTSYAML="$VOICEGER_DIR/GPT-SoVITS/GPT_SoVITS/configs/tts_infer.yaml"
    if [ -f "$TTSYAML" ]; then
        if command -v nvidia-smi &>/dev/null && nvidia-smi &>/dev/null 2>&1; then
            info "tts_infer.yaml を CUDA モードに設定中..."
            sed -i 's/^\(\s*device:\s*\)cpu/\1cuda/' "$TTSYAML"
            sed -i 's/^\(\s*is_half:\s*\)false/\1true/' "$TTSYAML"
            info "  device: cuda, is_half: true (FP16) を設定しました"
        else
            info "GPU が検出されませんでした。tts_infer.yaml は CPU モードのままです"
        fi
    fi

    # --- 起動コマンドの構築（direct Python path で conda activation 不要） ---
    VOICEGER_API="$VOICEGER_DIR/GPT-SoVITS/api_v2.py"
    VOICEGER_CMD="$VOICEGER_PYTHON $VOICEGER_API"
    VOICEGER_REF_AUDIO="$VOICEGER_DIR/reference/01_ref_emoNormal026.wav"
    VOICEGER_REF_TEXT=$(cat "$VOICEGER_DIR/reference/ref_text.txt" 2>/dev/null || echo "流し切りが完全に入ればデバフの効果が付与される")

    # --- config.tomlにVoiceger設定を書き込む ---
    update_or_append() {
        set_toplevel_key "$CONFIG_DIR/config.toml" "$1" "\"$2\""
    }
    update_or_append "voiceger_path"      "$VOICEGER_CMD"
    update_or_append "voiceger_ref_audio" "$VOICEGER_REF_AUDIO"
    update_or_append "voiceger_prompt_text" "$VOICEGER_REF_TEXT"
    update_or_append "voiceger_prompt_lang" "ja"

    info "Voicegerのインストールが完了しました: $VOICEGER_DIR"
    VOICEGER_INSTALLED=true
fi

# ---------- 完了 ----------
echo ""
echo -e "${GREEN}${BOLD}===== インストール完了！ =====${NC}"
echo ""
echo "アプリケーションメニューから「ZunduxTTS」を起動できます。"
echo "またはコマンドラインから: zundux_tts_launch.sh"
echo ""
if [ "$VOICEGER_INSTALLED" = true ]; then
    echo -e "${GREEN}Voiceger: インストール済み${NC}"
    echo "  設定 → Voiceger接続 から動作確認できます"
    echo "  起動コマンド: $VOICEGER_CMD"
    echo ""
fi
if [ "$NEED_RELOGIN" = true ]; then
    echo -e "${YELLOW}${BOLD}重要: dockerグループへの追加を反映するため、再ログインしてください。${NC}"
    echo ""
fi
