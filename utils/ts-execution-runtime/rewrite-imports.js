// This plugin transforms module imports inside `pnpm` codebase, so that they point to
// TypeScript sources `src/index.ts` and not to declared entrypoints `lib/index.js`.
// Also, sometimes the code uses deep imports from `@pnpm/.../lib/...` packages,
// the plugin rewrites `lib` to `src` for such deep imports to point back to TypeScript code

const path = require('path')

const NATIVE_MODULES = new Set(Object.keys(process.binding('natives')))

module.exports = ({ types: t }, opts) => {
  function rewriteModulePath(source, file, state) {
    const opts = state.opts
    let result = source
    const sourceParts = source.split('/')
    const isQualifiedModulePath = sourceParts.length > 2 || (sourceParts.length == 2 && sourceParts[0][0] !== '@')
    if (!isQualifiedModulePath && !path.isAbsolute(source) && !source.startsWith('.') && !NATIVE_MODULES.has(source)) {
      try {
        const packageRootDir = path.dirname(require.resolve(source + '/package.json', {paths: [file]}))
        if (packageRootDir.split(path.sep).indexOf('node_modules') < 0) {
          result = path.join(packageRootDir, 'src/index.ts')
        }
      } catch (e) {
        // If we have hit ESM module, we just ignore this case for now and do not rewrite imports
        if (e.code !== 'ERR_PACKAGE_PATH_NOT_EXPORTED')
          throw e;
      }
    }
    if (isQualifiedModulePath && source.startsWith('@pnpm/') && source.indexOf('/lib/') >= 0) {
      result = source.replace('/lib/', '/src/')
    }

    if (result !== source) {
      return result
    } else {
      return
    }
  }

  function replaceRequire(nodePath, state) {
    if (
      !t.isIdentifier(nodePath.node.callee, { name: 'require' }) &&
        !(
            t.isMemberExpression(nodePath.node.callee) &&
            t.isIdentifier(nodePath.node.callee.object, { name: 'require' })
        )
    ) {
      return
    }

    const moduleArg = nodePath.node.arguments[0]
    if (moduleArg && moduleArg.type === 'StringLiteral') {
      const modulePath = rewriteModulePath(moduleArg.value, state.file.opts.filename, state)
      if (modulePath) {
        nodePath.replaceWith(t.callExpression(
          nodePath.node.callee, [t.stringLiteral(modulePath)]
        ))
      }
    }
  }

  function replaceImportExport(nodePath, state) {
    const moduleArg = nodePath.node.source
    if (moduleArg && moduleArg.type === 'StringLiteral') {
      const modulePath = rewriteModulePath(moduleArg.value, state.file.opts.filename, state)
      if (modulePath) {
        nodePath.node.source = t.stringLiteral(modulePath)
      }
    }
  }

  return {
    visitor: {
      CallExpression: {
        exit(nodePath, state) {
          return replaceRequire(nodePath, state)
        }
      },
      ImportDeclaration: {
        exit(nodePath, state) {
          return replaceImportExport(nodePath, state)
        }
      },
      ExportDeclaration: {
        exit(nodePath, state) {
          return replaceImportExport(nodePath, state)
        }
      },
    }
  }
}
