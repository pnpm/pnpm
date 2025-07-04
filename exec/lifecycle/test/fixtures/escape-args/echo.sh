#!/usr/bin/env node

const fs = require('node:fs');
const path = require('node:path');

fs.writeFileSync(path.join(__dirname, 'output.json'), JSON.stringify(process.argv.slice(2), null, 2))
