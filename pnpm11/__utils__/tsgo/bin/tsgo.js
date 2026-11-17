#!/usr/bin/env node
// Runs the TypeScript 7 compiler under the name `tsgo`. The workspace root also
// depends on TypeScript 6 for typescript-eslint, and both releases publish a
// `tsc` bin, so the root `tsc` would be ambiguous.
import { createRequire } from 'node:module'
import path from 'node:path'
import { pathToFileURL } from 'node:url'

const typescriptDir = path.dirname(createRequire(import.meta.url).resolve('typescript/package.json'))
await import(pathToFileURL(path.join(typescriptDir, 'lib/tsc.js')).href)
