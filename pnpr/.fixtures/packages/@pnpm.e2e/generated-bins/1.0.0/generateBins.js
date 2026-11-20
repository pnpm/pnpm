'use strict'
const fs = require('fs')
const path = require('path')

const bin = path.join(__dirname, 'bin')

fs.mkdirSync(bin)
fs.writeFileSync(path.join(bin, 'cmd1'), '#!/usr/bin/env node', 'utf8')
fs.writeFileSync(path.join(bin, 'cmd2'), '#!/usr/bin/env node', 'utf8')
