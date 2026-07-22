FROM debian:bookworm

# 安装 Tauri/Linux 交叉编译所需的系统库和工具链
RUN apt-get update && apt-get install -y \
    # Tauri 依赖
    libwebkit2gtk-4.1-dev \
    libgtk-3-dev \
    libappindicator3-dev \
    librsvg2-dev \
    libssl-dev \
    libglib2.0-dev \
    pkg-config \
    libayatana-appindicator3-dev \
    # 交叉编译工具链
    gcc-x86-64-linux-gnu \
    g++-x86-64-linux-gnu \
    libc6-dev-amd64-cross \
    cross-binutils-common \
    # Node.js 和 pnpm
    wget \
    curl \
    && rm -rf /var/lib/apt/lists/*

# 安装 Node.js
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y nodejs \
    && npm install -g pnpm

# 设置交叉编译环境变量
ENV CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc \
    CXX_x86_64_unknown_linux_gnu=x86_64-linux-gnu-g++ \
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc
