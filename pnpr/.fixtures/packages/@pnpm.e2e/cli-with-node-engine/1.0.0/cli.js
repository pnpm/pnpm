#!/usr/bin/env node
import fs from 'node:fs'

fs.writeFileSync('node-version', process.version, 'utf8')
