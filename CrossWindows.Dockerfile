FROM ghcr.io/cross-rs/x86_64-pc-windows-gnu:0.2.5

RUN apt-get update && apt-get install -y \
    wget \
    && rm -rf /var/lib/apt/lists/*

# 安装 pnpm 和 Node.js（用于前端构建）
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y nodejs \
    && npm install -g pnpm
