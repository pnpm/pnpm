# pnpm-agent — server-side dependency resolution + file delivery for pnpm clients.
#
# Build:  docker build -t pnpm-agent .
# Run:    docker run -p 4873:4873 -v $(pwd)/agent-data:/agent-data pnpm-agent

FROM ghcr.io/pnpm/pnpm

# The base image ships pnpm on PATH (PNPM_HOME=/pnpm) but no Node runtime.
# Install one with pnpm's runtime manager, then install pnpm-agent itself
# into the same global bin dir so both `node` and `pnpm-agent` resolve.
RUN pnpm runtime set node 24 -g \
 && pnpm add -g pnpm-agent

# Server config (override at runtime with `docker run -e`).
ENV PORT=4873 \
    PNPM_AGENT_STORE_DIR=/agent-data/store \
    PNPM_AGENT_CACHE_DIR=/agent-data/cache \
    PNPM_AGENT_UPSTREAM=https://registry.npmjs.org/

EXPOSE 4873
VOLUME ["/agent-data"]

CMD ["pnpm-agent"]
