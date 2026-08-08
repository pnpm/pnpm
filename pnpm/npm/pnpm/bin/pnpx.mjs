#!/usr/bin/env node
import process from 'node:process'

import { runPnpm } from './pnpm.mjs'

runPnpm({ argv: ['dlx', ...process.argv.slice(2)] })
