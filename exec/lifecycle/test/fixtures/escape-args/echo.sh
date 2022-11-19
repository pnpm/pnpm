#!/usr/bin/env node

const fs = require('fs');
const path = require('path');

fs.writeFileSync(path.join(__dirname, 'output.json'), JSON.stringify(process.argv.slice(2), null, 2))
