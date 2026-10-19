import process from 'node:process'
import { packlist } from '../../../../../pnpm11/fs/packlist/lib/index.js'

process.stdin.setEncoding('utf8')
let input = ''
for await (const chunk of process.stdin) input += chunk
const results = []
for (const { root, manifest } of JSON.parse(input)) {
  results.push((await packlist(root, { manifest })).sort())
}
process.stdout.write(JSON.stringify(results))
