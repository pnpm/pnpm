#!/usr/bin/env node
// Wrapper for `@pnpm/registry-mock` that drives the server via the
// programmatic `start({ useNodeVersion, listen })` entry point
// instead of the bare CLI. Mirrors pnpm's own jest globalSetup at
// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/__utils__/jest-config/with-registry/globalSetup.js>.
//
// Why a wrapper: verdaccio 5.33 (the version `@pnpm/registry-mock@6`
// bundles) rejects its own auto-generated 64-character storage
// secret on Node 22+ with `Invalid storage secret key length, must
// be 32 characters long but is 64`. Pnpm pins
// `useNodeVersion: '20.16.0'` so verdaccio runs under a Node that
// skips that enforcement. The default CLI export of
// `@pnpm/registry-mock` does NOT pass `useNodeVersion` through, so
// the CLI launch fails on a modern host Node. The programmatic
// `start()` does.
//
// Pacquet's Rust launcher (`tasks/registry-mock/src/node_registry_mock.rs`)
// invokes this script via `node`. Calling pattern:
//   `node launch.mjs prepare`              — publish fixtures
//   `node launch.mjs` (or any other arg)   — launch the server; port
//                                            comes from
//                                            `PNPM_REGISTRY_MOCK_PORT`.

import { prepare, start } from '@pnpm/registry-mock'

const NODE_RUNTIME = '20.16.0'

if (process.argv[2] === 'prepare') {
  prepare()
  process.exit(0)
}

const listen = process.env.PNPM_REGISTRY_MOCK_PORT
if (!listen) {
  console.error('PNPM_REGISTRY_MOCK_PORT must be set when launching the mock')
  process.exit(1)
}

const server = start({
  useNodeVersion: NODE_RUNTIME,
  stdio: 'inherit',
  listen,
})

// Shell-convention exit code when the child was killed by a signal:
// 128 + signal number. Anything Node knows the number for; fall
// back to 1 otherwise.
const SIGNAL_NUMBERS = { SIGHUP: 1, SIGINT: 2, SIGTERM: 15 }

server.on('exit', (code, signal) => {
  if (signal != null) {
    process.exit(128 + (SIGNAL_NUMBERS[signal] ?? 1))
  } else {
    process.exit(code ?? 0)
  }
})

// Forward the usual termination signals to the child so it can shut
// down cleanly. The child's `exit` handler above is what actually
// terminates the wrapper — we do NOT re-raise the signal to ourselves
// (re-raising would either hit our own handler in a loop or, after
// removing it, race with the child's exit propagation).
for (const sig of Object.keys(SIGNAL_NUMBERS)) {
  process.on(sig, () => {
    if (!server.killed) server.kill(sig)
  })
}
