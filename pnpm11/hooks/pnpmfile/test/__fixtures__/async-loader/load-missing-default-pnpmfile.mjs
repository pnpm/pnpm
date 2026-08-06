import assert from 'node:assert/strict'

import { requireHooks } from '@pnpm/hooks.pnpmfile'

import { PROBE_SPECIFIER } from './loader.mjs'

// Fail loudly rather than passing vacuously if the loader stops being registered.
await assert.rejects(import(PROBE_SPECIFIER), /the asynchronous loader is registered/)

const { resolvedPnpmfilePaths } = await requireHooks(import.meta.dirname, { tryLoadDefaultPnpmfile: true })
assert.deepEqual(resolvedPnpmfilePaths, [])
