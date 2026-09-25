#!/usr/bin/env node
// Corepack's `pnpx` entry point; see ./pnpm.mjs. The native binary infers the
// `dlx` subcommand from the name it was launched under, which does not survive
// being spawned from here, so inject it the way the `pnpx` shell script does.
import process from 'node:process'

process.argv = [...process.argv.slice(0, 2), 'dlx', ...process.argv.slice(2)]

await import('./pnpm.mjs')
