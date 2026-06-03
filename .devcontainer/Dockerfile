FROM ubuntu:24.04

WORKDIR /workspace

# Install build dependencies
RUN apt-get update && apt-get upgrade -y && apt-get install -y \
    apt-utils \
    curl \
    git \
    git-lfs \
    gnupg2 \
    jq \
    build-essential \
    openssl \
    libssl-dev \
    pkg-config \
    cmake \
    wget \
    file \
    ca-certificates \
    zstd \
    clang \
    lld \
    protobuf-compiler \
    seccomp \
    libseccomp-dev \
    && rm -rf /var/lib/apt/lists/*

# Install Rust
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y
ENV PATH="/root/.cargo/bin:$PATH"

# Init git-lfs
RUN git lfs install

ENV TZ=Asia/Shanghai
ENV LANG=en_US.UTF-8
ENV LC_ALL=en_US.UTF-8
ENV LIBCLANG_PATH=/usr/lib/llvm-18/lib
