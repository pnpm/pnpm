import fs = require('fs')
import path = require('path')
import proxiquire = require('proxyquire')
import sinon = require('sinon')
import test = require('tape')

const fsMock = {} as any // tslint:disable-line
const makeDirMock = sinon.spy()
const createImportPackage = proxiquire('@pnpm/package-store/lib/storeController/createImportPackage', {
  '../fs/importIndexedDir': proxiquire('@pnpm/package-store/lib/fs/importIndexedDir', {
    'make-dir': makeDirMock,
    'mz/fs': fsMock,
    'path-temp': (dir: string) => path.join(dir, '_tmp')
  }),
  'make-dir': makeDirMock,
  'mz/fs': fsMock,
}).default

test('packageImportMethod=auto: clone files by default', async (t) => {
  const importPackage = createImportPackage('auto')
  fsMock.copyFile = sinon.spy()
  fsMock.rename = sinon.spy()
  await importPackage('/store/package', '/project/package', {
    filesResponse: {
      filenames: ['package.json', 'index.js'],
      fromStore: false,
    },
    force: false,
  })
  t.ok(fsMock.copyFile.calledWith('/store/package/package.json', '/project/_tmp/package.json', fs.constants.COPYFILE_FICLONE_FORCE))
  t.ok(fsMock.copyFile.calledWith('/store/package/index.js', '/project/_tmp/index.js', fs.constants.COPYFILE_FICLONE_FORCE))
  t.end()
})

test('packageImportMethod=auto: link files if cloning fails', async (t) => {
  const importPackage = createImportPackage('auto')
  fsMock.copyFile = () => { throw new Error('This file system does not support cloning') }
  fsMock.link = sinon.spy()
  fsMock.rename = sinon.spy()
  await importPackage('/store/package', '/project/package', {
    filesResponse: {
      filenames: ['package.json', 'index.js'],
      fromStore: false,
    },
    force: false,
  })
  t.ok(fsMock.link.calledWith('/store/package/package.json', '/project/_tmp/package.json'))
  t.ok(fsMock.link.calledWith('/store/package/index.js', '/project/_tmp/index.js'))
  t.end()
})
