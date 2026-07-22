#!/usr/bin/env bash
set -euo pipefail

# ===== Tauri 跨平台构建脚本 =====
# 用法:
#   ./build.sh              # 构建当前平台
#   ./build.sh macos        # macOS
#   ./build.sh linux        # Linux (需要 Linux 环境)
#   ./build.sh windows      # Windows (需要 Windows 环境)
#   ./build.sh all          # 当前等同于 macOS 构建（仅 macOS）

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log_info()  { echo -e "${GREEN}[INFO]${NC} $1"; }
log_warn()  { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

PLATFORM="${1:-}"

# 检查 Docker 是否运行（cross 需要）
check_docker() {
    if ! docker info > /dev/null 2>&1; then
        log_error "Docker 未运行，请先启动 Docker Desktop"
        exit 1
    fi
}

# 检查 cross 是否安装
check_cross() {
    if ! command -v cross &> /dev/null; then
        log_error "cross 未安装，请运行: cargo install cross --git https://github.com/cross-rs/cross"
        exit 1
    fi
}

# 读取版本号
get_version() {
    grep '"version"' src-tauri/tauri.conf.json | sed 's/.*"version": "\([^"]*\)".*/\1/' | tr -d ' \r\n'
}

# 收集产物到版本目录
collect_artifacts() {
    local version="$1"
    local output_dir="releases/${version}"
    local platform="$2"

    mkdir -p "${output_dir}/${platform}"

    case "$platform" in
        darwin)
            local dmg=$(find src-tauri/target/release/bundle/dmg -name "*.dmg" 2>/dev/null | head -1)
            [[ -n "$dmg" ]] && cp "$dmg" "${output_dir}/${platform}/"

            local app_dir=$(find src-tauri/target/release/bundle/app -name "*.app" 2>/dev/null | head -1)
            if [[ -n "$app_dir" ]]; then
                tar -czf "${output_dir}/${platform}/$(basename "$app_dir").tar.gz" -C "$(dirname "$app_dir")" "$(basename "$app_dir")"
            fi
            ;;
        linux)
            local appimage=$(find src-tauri/target/x86_64-unknown-linux-gnu/release/bundle/appimage -name "*.AppImage" 2>/dev/null | head -1)
            [[ -n "$appimage" ]] && cp "$appimage" "${output_dir}/${platform}/"

            local deb=$(find src-tauri/target/x86_64-unknown-linux-gnu/release/bundle/deb -name "*.deb" 2>/dev/null | head -1)
            [[ -n "$deb" ]] && cp "$deb" "${output_dir}/${platform}/"
            ;;
        windows)
            local exe=$(find src-tauri/target/x86_64-pc-windows-gnu/release/bundle/nsis -name "*.exe" 2>/dev/null | head -1)
            [[ -n "$exe" ]] && cp "$exe" "${output_dir}/${platform}/"

            local msi=$(find src-tauri/target/x86_64-pc-windows-gnu/release/bundle -name "*.msi" 2>/dev/null | head -1)
            [[ -n "$msi" ]] && cp "$msi" "${output_dir}/${platform}/"
            ;;
    esac

    log_info "${platform} 产物已收集到 ${output_dir}/${platform}/"
}

# macOS 原生构建
build_macos() {
    log_info "开始 macOS 原生构建..."
    pnpm install --frozen-lockfile
    # CI mode makes Tauri skip Finder AppleScript, which is unreliable in
    # headless shells while producing the same installable DMG contents.
    CI=true pnpm tauri build --bundles app,dmg
    log_info "macOS 构建完成 ✓"
}

# Linux 交叉编译 (Rust 代码)
build_linux_cross() {
    log_info "开始 Linux x86_64 交叉编译 (仅 Rust 代码)..."
    check_docker
    check_cross
    pnpm install --frozen-lockfile
    pnpm build
    cross build --release --manifest-path src-tauri/Cargo.toml --target x86_64-unknown-linux-gnu
    log_info "Linux Rust 代码编译完成 ✓"
    log_warn "提示: 如需 .AppImage/.deb 安装包，请在 Linux 原生环境运行 ./build.sh linux"
}

# Windows 交叉编译 (Rust 代码)
build_windows_cross() {
    log_info "开始 Windows x86_64 交叉编译 (仅 Rust 代码)..."
    check_docker
    check_cross
    pnpm install --frozen-lockfile
    pnpm build
    cross build --release --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-gnu
    log_info "Windows Rust 代码编译完成 ✓"
    log_warn "提示: 如需 .exe/.msi 安装包，请在 Windows 原生环境运行 ./build.sh windows"
}

# Linux 原生完整构建
build_linux_native() {
    log_info "开始 Linux 原生完整构建..."
    pnpm install --frozen-lockfile
    pnpm tauri build --bundles deb,appimage
    log_info "Linux 构建完成 ✓"
}

# Windows 原生完整构建
build_windows_native() {
    log_info "开始 Windows 原生完整构建..."
    pnpm install --frozen-lockfile
    pnpm tauri build --bundles nsis
    log_info "Windows 构建完成 ✓"
}

# 构建所有平台
build_all() {
    local version
    version=$(get_version)
    local os="$(uname -s)"

    if [[ "$os" == "Darwin" ]]; then
        build_macos
        collect_artifacts "$version" "darwin"
        log_warn "Linux/Windows 交叉编译仅生成 Rust 二进制文件，不包含安装包"
        log_warn "请在对应平台运行 ./build.sh linux 或 ./build.sh windows 获取完整安装包"
    else
        log_error "all 模式仅支持 macOS 运行"
        exit 1
    fi


    log_info "=========================================="
    log_info "构建完成！版本: $version"
    log_info "产物目录: releases/${version}/"
    log_info "=========================================="
}

case "$PLATFORM" in
    macos|mac|macOS)
        version=$(get_version)
        build_macos
        collect_artifacts "$version" "darwin"
        ;;
    linux)
        version=$(get_version)
        os="$(uname -s)"
        if [[ "$os" == "Linux" ]]; then
            build_linux_native
            collect_artifacts "$version" "linux"
        else
            build_linux_cross
            collect_artifacts "$version" "linux"
        fi
        ;;
    windows)
        version=$(get_version)
        os="$(uname -s)"
        if [[ "$os" == *"MINGW"* || "$os" == *"MSYS"* || "$os" == *"CYGWIN"* ]]; then
            build_windows_native
        else
            build_windows_cross
        fi
        collect_artifacts "$version" "windows"
        ;;
    all)
        build_all
        ;;
    "")
        version=$(get_version)
        os="$(uname -s)"
        case "$os" in
            Darwin)  build_macos; collect_artifacts "$version" "darwin" ;;
            Linux)   build_linux_native; collect_artifacts "$version" "linux" ;;
            MINGW*|MSYS*|CYGWIN*) build_windows_native; collect_artifacts "$version" "windows" ;;
            *) log_error "未知平台: $os"; exit 1 ;;
        esac
        ;;
    *)
        log_error "未知目标: $PLATFORM"
        echo "用法: $0 [macos|linux|windows|all]"
        exit 1
        ;;
esac

log_info "产物目录: releases/${version}/"
