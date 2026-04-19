# pnpm-agent — server-side dependency resolution + file delivery for pnpm clients.
#
# Build:  docker build -t pnpm-agent .
# Run:    docker run -p 4873:4873 -v $(pwd)/agent-data:/agent-data pnpm-agent

FROM node:22-slim

# Install pnpm via corepack — pinned for reproducibility.
RUN corepack enable && corepack prepare pnpm@latest --activate

# `pnpm add -g` installs the binary into PNPM_HOME; put it on PATH.
ENV PNPM_HOME=/usr/local/share/pnpm
ENV PATH=$PNPM_HOME:$PATH

RUN pnpm add -g pnpm-agent

# Server config (override at runtime with `docker run -e`).
ENV PORT=4873 \
    PNPM_AGENT_STORE_DIR=/agent-data/store \
    PNPM_AGENT_CACHE_DIR=/agent-data/cache \
    PNPM_AGENT_UPSTREAM=https://registry.npmjs.org/

EXPOSE 4873
VOLUME ["/agent-data"]

CMD ["pnpm-agent"]
