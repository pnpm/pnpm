#! /usr/bin/env node
const nodeInfo = {
  execPath: process.execPath,
  versions: process.versions,
}
console.log(JSON.stringify(nodeInfo, undefined, 2))
