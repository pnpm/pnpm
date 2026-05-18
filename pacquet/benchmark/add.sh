#!/usr/bin/env bash

# Make sure to create a .npmrc file
# registry=http://localhost:4873

PACQUET="../target/release/pacquet add fastify"
PNPM="pnpm add fastify --silent"
YARN="yarn add fastify --silent"
BUN="bun add fastify --no-cache --silent --backend=symlink"

FILE_CLEAN="rm -rf package.json node_modules .yarn yarn.lock .pnp* && echo {} > package.json || true"
PACQUET_CLEAN="../target/release/pacquet store prune"
PNPM_CLEAN="pnpm store prune"
YARN_CLEAN="yarn cache clean --all"
CLEANUP="${PACQUET_CLEAN} && ${PNPM_CLEAN} && ${YARN_CLEAN} && ${FILE_CLEAN}"

$FILE_CLEAN

hyperfine -w 5 -i \
  --prepare "${CLEANUP}" \
  -n pacquet "${PACQUET}" \
  -n pnpm "${PNPM}" \
  -n yarn "${YARN}" \
  -n bun "${BUN}"

$FILE_CLEAN
