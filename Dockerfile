# syntax=docker/dockerfile:1.7
#
# The officexr image: web app, studio and bots, with every layout pre-baked
# and the KayKit asset packs installed. `pnpm --filter @officexr/server start`
# runs the web backend (the built SPA in packages/web/dist is served
# elsewhere); the studio deployment overrides the command with
# `pnpm --filter @officexr/studio-server start`.
#
# Stages are ordered so the slow ones key only on what actually changes them
# and come back from the layer cache otherwise:
#   store   — pnpm's package store, keyed on the lockfile alone
#   packs   — the asset packs, keyed on their pinned URLs and checksums
#   browser — Chromium's headless shell, keyed on the playwright-core version
#   sim     — the @officexr/sim-wasm and @officexr/render-wasm packages, keyed on crates/ and the toolchain
#   build   — source, install (offline, from the store), bake and build

ARG NODE_IMAGE=node:22.22.3-bookworm-slim
# Must equal packages/studio-mcp's playwright-core; the runtime stage checks it.
ARG PLAYWRIGHT_CORE_VERSION=1.60.0
# Keep in step with .github/actions/build-wasm.
ARG WASM_PACK_VERSION=0.15.1
ARG WASM_PACK_SHA256=410101009739b9d01c7d5affaaad6e9c9a24b4cba6791a4805db8e89b9bf7af2

FROM ${NODE_IMAGE} AS store
ENV PNPM_HOME=/pnpm
ENV npm_config_store_dir=/pnpm/store
RUN corepack enable
WORKDIR /app
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
RUN corepack install && pnpm fetch

FROM alpine:3.22 AS packs
# ponytail: the pack URLs also live in packages/world/scripts/download-asset-packs.ts; keep them in step.
ADD --checksum=sha256:e6d75f34c5545486b5a8f45f86209dbd49092f3e77ae23011bdb87edab89e7e4 \
  https://pub-5c607701fd464418972bca5504147874.r2.dev/KayKit_Furniture_Bits_1.0_FREE.zip /zips/furniture.zip
ADD --checksum=sha256:6dadbcd7dd13ba8582323cf3cfdec591340f83f74a959c212f8b83eb8dc42425 \
  https://pub-5c607701fd464418972bca5504147874.r2.dev/KayKit_Prototype_Bits_1.1_FREE.zip /zips/prototype.zip
ADD --checksum=sha256:75f75f0a1ae569f3f2348fa7a04ccd45d2f010d419045755c70ae43289b68e4b \
  https://pub-5c607701fd464418972bca5504147874.r2.dev/KayKit_Restaurant_Bits_1.0_FREE.zip /zips/restaurant.zip
RUN for pack in furniture prototype restaurant; do \
      mkdir -p "/models/$pack" && unzip -q "/zips/$pack.zip" -d "/models/$pack"; \
    done

FROM ${NODE_IMAGE} AS browser
ARG PLAYWRIGHT_CORE_VERSION
ENV PLAYWRIGHT_BROWSERS_PATH=/ms-playwright
# A stable path to the binary, whose build directory is revision-numbered.
RUN npx -y "playwright-core@${PLAYWRIGHT_CORE_VERSION}" install --only-shell chromium && \
    ln -s "$(find /ms-playwright -type f -name chrome-headless-shell)" /ms-playwright/chrome-headless-shell

FROM rust:1.100.0-bookworm AS sim
ARG WASM_PACK_VERSION
ARG WASM_PACK_SHA256
RUN apt-get update && \
    apt-get install -y --no-install-recommends clang lld && \
    rm -rf /var/lib/apt/lists/* && \
    curl -sSfL -o /tmp/wasm-pack.tgz "https://github.com/rustwasm/wasm-pack/releases/download/v${WASM_PACK_VERSION}/wasm-pack-v${WASM_PACK_VERSION}-x86_64-unknown-linux-musl.tar.gz" && \
    echo "${WASM_PACK_SHA256}  /tmp/wasm-pack.tgz" | sha256sum -c - && \
    tar -xzf /tmp/wasm-pack.tgz --strip-components=1 -C /usr/local/cargo/bin "wasm-pack-v${WASM_PACK_VERSION}-x86_64-unknown-linux-musl/wasm-pack" && \
    rm /tmp/wasm-pack.tgz
WORKDIR /app
COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
COPY .cargo .cargo
RUN rustup toolchain install
COPY crates crates
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    wasm-pack build crates/officexr-sim-wasm --release --target web --no-pack --out-dir /app/packages/sim-wasm/pkg && \
    for backend in webgpu webgl2; do \
      wasm-pack build crates/officexr-render-web --profile wasm-release --target web --no-pack --out-dir /app/packages/render-wasm/pkg/$backend -- --features $backend || exit 1; \
    done

FROM store AS build
COPY . .
COPY --from=sim /app/packages/sim-wasm/pkg packages/sim-wasm/pkg
COPY --from=sim /app/packages/render-wasm/pkg packages/render-wasm/pkg
RUN pnpm install --offline --frozen-lockfile \
      --filter @officexr/web... --filter @officexr/studio... --filter @officexr/studio-server... \
      --filter @officexr/world...
COPY --from=packs /models packages/studio/public/models
# Baked layouts are gitignored, so bake them all here; the studio seeds its
# data dir from packages/world/baked-layouts/, and vite copies public/ (with
# the packs) into the studio's dist.
RUN pnpm --filter @officexr/world bake:layout --all && \
    pnpm --filter @officexr/web build && \
    pnpm --filter @officexr/studio build

FROM ${NODE_IMAGE}
ARG PLAYWRIGHT_CORE_VERSION
# ca-certificates: the web pods resolve secrets over TLS at startup.
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    npx -y "playwright-core@${PLAYWRIGHT_CORE_VERSION}" install-deps chromium-headless-shell && \
    rm -rf /var/lib/apt/lists/* /root/.npm
RUN corepack enable
COPY --from=browser /ms-playwright /ms-playwright
COPY --from=build --chown=node:node /app /app
ENV OFFICEXR_RENDER_CHROMIUM_PATH=/ms-playwright/chrome-headless-shell
WORKDIR /app
USER node
# Fails the build if studio-mcp's playwright-core drifted from the browser
# installed above, or the browser needs a library install-deps didn't add.
RUN test "$(node -p "require('/app/packages/studio-mcp/node_modules/playwright-core/package.json').version")" = "${PLAYWRIGHT_CORE_VERSION}" && \
    "${OFFICEXR_RENDER_CHROMIUM_PATH}" --version
# Pinned so this step and the running container agree on the cache path, which
# holds the packageManager pnpm so containers start offline.
ENV COREPACK_HOME=/home/node/.corepack
RUN corepack install
ENV COREPACK_ENABLE_NETWORK=0
CMD ["pnpm", "--filter", "@officexr/server", "start"]
