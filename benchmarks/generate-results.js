const fs = require('fs')

const benchDir = process.argv[2]
const outputFile = process.argv[3]

const benchmarks = [
  ['headless', 'Headless (warm store+cache)'],
  ['peek', 'Re-resolution (add dep, warm)'],
  ['nolockfile', 'Full resolution (warm, no lockfile)'],
  ['headless-cold', 'Headless (cold store+cache)'],
  ['cold', 'Cold install (nothing warm)'],
]

function readResult (benchDir, name, variant) {
  try {
    const data = JSON.parse(fs.readFileSync(`${benchDir}/${name}-${variant}.json`, 'utf8'))
    const r = data.results[0]
    return `${r.mean.toFixed(3)}s ± ${r.stddev.toFixed(3)}s`
  } catch (err) {
    if (err && err.code !== 'ENOENT') {
      console.error(`Warning: failed to read ${name}-${variant}: ${err.message}`)
    }
    return 'n/a'
  }
}

const lines = [
  '# Benchmark Results',
  '',
  '| # | Scenario | main | branch |',
  '|---|---|---|---|',
]

benchmarks.forEach(([name, label], i) => {
  const mainCell = readResult(benchDir, name, 'main')
  const branchCell = readResult(benchDir, name, 'branch')
  lines.push(`| ${i + 1} | ${label} | ${mainCell} | ${branchCell} |`)
})

lines.push('')

const output = lines.join('\n')
fs.writeFileSync(outputFile, output)
console.log(output)
