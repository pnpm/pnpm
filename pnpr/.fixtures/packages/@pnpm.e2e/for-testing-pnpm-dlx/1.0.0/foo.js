#!/usr/bin/env node
const fs = require('fs')

fs.writeFileSync('foo', '', 'utf8')
