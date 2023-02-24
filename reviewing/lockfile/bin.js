const args = process.argv.slice(2);


if (args.length < 2) {
  console.log(`
lockfile-review [target-lockfile] [current-lockfile]
lr [target-lockfile] [current-lockfile]

Options:
  --path:       Show introduction path
  --workspace:   Differentiation by workspace
`);
  process.exit(0);
}

const path = require('path');
const fs = require('fs/promises');
const yaml = require('js-yaml');
const chalk = require('chalk');
const { createLockfileGraph, diffGraph } = require('./lib/index');

async function readTask() {
  const targetPath = args[0];
  const currentPath = args[1];
  async function task(p) {
    return fs.readFile(path.resolve(p)).catch(() =>
      fs.readFile(path.resolve(path.dirname(p), 'pnpm-lock.yaml'))
    ).then(content => yaml.load(content.toString('utf-8')))
  }
  const [target, current] = await Promise.all([task(targetPath), task(currentPath)])
  return { target, current }
}

async function run() {
  const { target, current } = await readTask();
  const targetGraph = createLockfileGraph(target);
  const currentGraph = createLockfileGraph(current);
  const diff = diffGraph(targetGraph, currentGraph);

  const showPaths = args.includes('--paths');
  const showWorkspace = args.includes('--workspace')
  if (!showPaths && !showWorkspace) {
    const added = diff.packages.added.map((key) => {
      return chalk.green(`+ ${key}`)
    });
    const deleted = diff.packages.deleted.map((key) => {
      return chalk.red(`- ${key}`)
    });
    const changed = [...deleted, ...added];
    if (changed.length > 0) {
      console.log(changed.join('\n'))
    }
    return;
  }

  if (showPaths && showWorkspace) {
    const changed = {};
    diff.packages.deleted.forEach(key => {
      targetGraph.whoImportThisPackage(key).filter(Boolean).forEach(paths => {
        const importerId = paths[0];
        if (!targetGraph.getImporter(importerId)) {
          return;
        }
        if (!changed[importerId]) {
          changed[importerId] = {}
        }
        if (!changed[importerId][key]) {
          changed[importerId][key] = [`  ${chalk.red(`- ${key}`)}:`]
        }
        changed[importerId][key].push(`      ${paths.join(' -> ')}`)
      })
    })
    diff.packages.added.forEach(key => {
      currentGraph.whoImportThisPackage(key).filter(Boolean).forEach(paths => {
        const importerId = paths[0];
        if (!currentGraph.getImporter(importerId)) {
          return;
        }
        if (!changed[importerId]) {
          changed[importerId] = {}
        }
        if (!changed[importerId][key]) {
          changed[importerId][key] = [`  ${chalk.green(`+ ${key}`)}:`]
        }
        changed[importerId][key].push(`      ${paths.join(' -> ')}`)
      })
    })

    for (const importerId in changed) {
      console.log(`${importerId}:`);
      for (const key in changed[importerId]) {
        console.log(changed[importerId][key].join('\n'))
      }
    }

  } else if (showWorkspace) {
    const changed = {};
    diff.packages.deleted.forEach(key => {
      targetGraph.whoImportThisPackage(key).filter(Boolean).forEach(([importerId]) => {
        if (!targetGraph.getImporter(importerId)) {
          return;
        }
        if (!changed[importerId]) {
          changed[importerId] = new Set()
        }
        changed[importerId].add(`  ${chalk.red(`- ${key}`)}`)
      })
    });
    diff.packages.added.forEach(key => {
      currentGraph.whoImportThisPackage(key).filter(Boolean).forEach(([importerId]) => {
        if (!currentGraph.getImporter(importerId)) {
          return;
        }
        if (!changed[importerId]) {
          changed[importerId] = new Set();
        }
        changed[importerId].add(`  ${chalk.green(`+ ${key}`)}`)
      })
    });
    for (const importerId in changed) {
      console.log(`${importerId}:`);
      console.log(Array.from(changed[importerId]).join('\n'))
    }
  } else if (showPaths) {
    const added = diff.packages.added.map(key => {
      const paths = currentGraph.whoImportThisPackage(key).filter(Boolean).map((item => item.join(' -> ')));
      return [chalk.green(`+ ${key}:`), ...paths].join('\n  ');
    })
    const deleted = diff.packages.deleted.map(key => {
      const paths = targetGraph.whoImportThisPackage(key).filter(Boolean).map((item => item.join(' -> ')));
      return [chalk.red(`- ${key}:`), ...paths].join('\n  ');
    })
    const changed = [...deleted, ...added];
    if (changed.length > 0) {
      console.log(changed.join('\n'))
    }
  }
}

run();
