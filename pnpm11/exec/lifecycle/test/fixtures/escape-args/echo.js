#!/usr/bin/env node

import fs from 'fs'
import path from 'path'

fs.writeFileSync(path.join(import.meta.dirname, 'output.json'), JSON.stringify(process.argv.slice(2), null, 2))
