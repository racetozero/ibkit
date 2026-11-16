# syntax=docker/dockerfile:1
FROM --platform=$BUILDPLATFORM rust:1-bookworm AS build
ARG TARGETARCH
ARG BUILDARCH
WORKDIR /build
RUN set -eu; \
    case "$TARGETARCH" in amd64) rust_target=x86_64-unknown-linux-gnu ;; arm64) rust_target=aarch64-unknown-linux-gnu ;; *) exit 1 ;; esac; \
    if [ "$TARGETARCH" != "$BUILDARCH" ]; then \
      case "$TARGETARCH" in amd64) packages="gcc-x86-64-linux-gnu libc6-dev-amd64-cross" ;; arm64) packages="gcc-aarch64-linux-gnu libc6-dev-arm64-cross" ;; esac; \
      apt-get update; apt-get install -y --no-install-recommends $packages; \
    fi; \
    rustup target add "$rust_target"; mkdir /out; echo "$rust_target" > /out/rust-target
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN set -eu; rust_target=$(cat /out/rust-target); \
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
      cargo build --release --locked -p ibkit-cli --target "$rust_target"; \
    cp "target/$rust_target/release/ibkit" "target/$rust_target/release/libibkit_gateway_agent.so" /out/

FROM build AS test-build
RUN set -eu; rust_target=$(cat /out/rust-target); \
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
      cargo test --locked --target "$rust_target" -p ibkit-cli --test jvm --no-run; \
    find "target/$rust_target/debug/deps" -maxdepth 1 -type f -name 'jvm-*' -executable -exec cp '{}' /out/jvm-tests \;

FROM debian:bookworm-slim AS gateway
ARG TARGETARCH
ARG GATEWAY_CHANNEL=latest
# Optional SHA-256 of the installer, per architecture (GATEWAY_SHA256 applies
# to both); the release build sets them, so a changed installer fails.
ARG GATEWAY_SHA256=""
ARG GATEWAY_SHA256_AMD64=""
ARG GATEWAY_SHA256_ARM64=""
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
RUN set -eu; case "$TARGETARCH" in amd64) vendor_arch=x64 ;; arm64) vendor_arch=arm ;; *) echo 'Use amd64 or arm64 for the Gateway image.' >&2; exit 1 ;; esac; \
    case "$GATEWAY_CHANNEL" in latest|stable) ;; *) echo 'Use latest or stable for GATEWAY_CHANNEL.' >&2; exit 1 ;; esac; \
    curl -fL --retry 3 "https://download2.interactivebrokers.com/installers/ibgateway/${GATEWAY_CHANNEL}-standalone/ibgateway-${GATEWAY_CHANNEL}-standalone-linux-${vendor_arch}.sh" -o /tmp/gateway.sh; \
    sha256sum /tmp/gateway.sh > /tmp/gateway-installer.sha256; \
    case "$TARGETARCH" in amd64) wanted=${GATEWAY_SHA256_AMD64:-$GATEWAY_SHA256} ;; arm64) wanted=${GATEWAY_SHA256_ARM64:-$GATEWAY_SHA256} ;; esac; \
    if [ -n "$wanted" ]; then echo "$wanted  /tmp/gateway.sh" | sha256sum -c - \
      || { echo "The Gateway installer changed: its SHA-256 is not $wanted. Check IBKR's release, then update the hash." >&2; exit 1; }; fi; \
    sh /tmp/gateway.sh -q -dir /opt/ibgateway; \
    cp /tmp/gateway-installer.sha256 /opt/ibgateway/installer.sha256; \
    rm /tmp/gateway.sh

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates tini xvfb x11vnc xauth libxext6 libxi6 libxrender1 libxtst6 libxrandr2 libxt6 libasound2 libfreetype6 libfontconfig1 fonts-dejavu-core libgtk-3-0 tzdata procps \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --create-home --home-dir /home/gateway gateway \
    && install -d -m 0700 -o gateway -g gateway /var/lib/ibkit
COPY --from=gateway --chown=gateway:gateway /opt/ibgateway /opt/ibgateway
COPY --from=build /out/ibkit /usr/local/bin/ibkit
COPY --from=build /out/libibkit_gateway_agent.so /usr/local/lib/libibkit_gateway_agent.so
COPY crates/ibkit-gateway/config.example.toml /etc/ibkit/config.toml
ENV IBKIT_CONFIG=/etc/ibkit/config.toml TZ=America/New_York
USER 10001:10001
WORKDIR /var/lib/ibkit
# 4004: IB API forwarder. 5800: control API, when IBKIT_CONTROL_LISTEN=0.0.0.0:5800
# and IBKIT_CONTROL_TOKEN are set.
EXPOSE 4004 5800
STOPSIGNAL SIGTERM
HEALTHCHECK --interval=30s --timeout=5s --start-period=15m --retries=3 CMD ["ibkit", "gateway", "health", "--kind", "ready"]
ENTRYPOINT ["/usr/bin/tini", "-g", "--", "/usr/local/bin/ibkit", "gateway"]
CMD ["run"]

# The optional verification image runs the production native library in a JDK.
FROM eclipse-temurin:17-jdk-noble AS verify
RUN apt-get update && apt-get install -y --no-install-recommends xvfb xauth tini libxtst6 libxi6 libxrender1 libfreetype6 libfontconfig1 fonts-dejavu-core && rm -rf /var/lib/apt/lists/*
COPY --from=test-build /out/jvm-tests /usr/local/bin/jvm-tests
COPY --from=build /out/libibkit_gateway_agent.so /out/libibkit_gateway_agent.so
COPY crates/ibkit-cli/tests/GatewayFixture.java /build/crates/ibkit-cli/tests/GatewayFixture.java
ENV IBKIT_AGENT_LIBRARY=/out/libibkit_gateway_agent.so
WORKDIR /build
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["xvfb-run", "-a", "/usr/local/bin/jvm-tests", "--include-ignored", "--nocapture"]

FROM runtime AS final
