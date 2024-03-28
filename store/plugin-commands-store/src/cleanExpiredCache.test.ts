import fs from 'fs'
import path from 'path'
import { createBase32Hash } from '@pnpm/crypto.base32-hash'
import { prepareEmpty } from '@pnpm/prepare'
import { cleanExpiredCache } from './cleanExpiredCache'

afterEach(() => {
  jest.restoreAllMocks()
})

test('cleanExpiredCache removes directories that outlive dlxCacheMaxAge', async () => {
  prepareEmpty()

  const cacheDir = path.resolve('cache')
  const dlxCacheMaxAge = 7
  const now = new Date()

  const timeTable = {
    foo: 12,
    bar: 1,
    baz: -20,
  }

  for (const [key, minuteDelta] of Object.entries(timeTable)) {
    const dirName = createBase32Hash(key)
    fs.mkdirSync(path.join(cacheDir, 'dlx', dirName, 'node_modules', '.pnpm'), { recursive: true })
    fs.mkdirSync(path.join(cacheDir, 'dlx', dirName, 'node_modules', '.bin'), { recursive: true })
    fs.writeFileSync(path.join(cacheDir, 'dlx', dirName, 'node_modules', '.modules.yaml'), '')
    fs.writeFileSync(path.join(cacheDir, 'dlx', dirName, 'package.json'), '')
    fs.writeFileSync(path.join(cacheDir, 'dlx', dirName, 'pnpm-lock.yaml'), '')
    const newDate = new Date(now.getTime() + minuteDelta * 60_000)
    fs.utimesSync(path.join(cacheDir, 'dlx', dirName), newDate, newDate)
  }

  const readdirSpy = jest.spyOn(fs.promises, 'readdir')
  const statSpy = jest.spyOn(fs.promises, 'stat')
  const rmSpy = jest.spyOn(fs.promises, 'rm')

  await cleanExpiredCache({
    cacheDir,
    dlxCacheMaxAge,
    now,
  })

  expect(
    fs.readdirSync(path.join(cacheDir, 'dlx'))
      .sort()
  ).toStrictEqual(
    ['foo', 'bar']
      .map(createBase32Hash)
      .sort()
  )

  expect(readdirSpy).toHaveBeenCalledWith(path.join(cacheDir, 'dlx'), expect.anything())
  for (const key of ['foo', 'bar', 'baz']) {
    expect(statSpy).toHaveBeenCalledWith(path.join(cacheDir, 'dlx', createBase32Hash(key)))
  }
  expect(rmSpy).toHaveBeenCalledWith(path.join(cacheDir, 'dlx', createBase32Hash('baz')), { recursive: true })

  readdirSpy.mockRestore()
  statSpy.mockRestore()
  rmSpy.mockRestore()
})

test('cleanExpiredCache removes all directories without checking stat if dlxCacheMaxAge is 0', async () => {
  prepareEmpty()

  const cacheDir = path.resolve('cache')
  const dlxCacheMaxAge = 0
  const now = new Date()

  const timeTable = {
    foo: 12,
    bar: 1,
    baz: -20,
  }

  for (const [key, minuteDelta] of Object.entries(timeTable)) {
    const dirName = createBase32Hash(key)
    fs.mkdirSync(path.join(cacheDir, 'dlx', dirName, 'node_modules', '.pnpm'), { recursive: true })
    fs.mkdirSync(path.join(cacheDir, 'dlx', dirName, 'node_modules', '.bin'), { recursive: true })
    fs.writeFileSync(path.join(cacheDir, 'dlx', dirName, 'node_modules', '.modules.yaml'), '')
    fs.writeFileSync(path.join(cacheDir, 'dlx', dirName, 'package.json'), '')
    fs.writeFileSync(path.join(cacheDir, 'dlx', dirName, 'pnpm-lock.yaml'), '')
    const newDate = new Date(now.getTime() + minuteDelta * 60_000)
    fs.utimesSync(path.join(cacheDir, 'dlx', dirName), newDate, newDate)
  }

  const readdirSpy = jest.spyOn(fs.promises, 'readdir')
  const statSpy = jest.spyOn(fs.promises, 'stat')
  const rmSpy = jest.spyOn(fs.promises, 'rm')

  await cleanExpiredCache({
    cacheDir,
    dlxCacheMaxAge,
    now,
  })

  expect(
    fs.readdirSync(path.join(cacheDir, 'dlx'))
      .sort()
  ).toStrictEqual([])

  expect(readdirSpy).toHaveBeenCalledWith(path.join(cacheDir, 'dlx'), expect.anything())
  expect(statSpy).not.toHaveBeenCalled()
  for (const key of ['foo', 'bar', 'baz']) {
    expect(rmSpy).toHaveBeenCalledWith(path.join(cacheDir, 'dlx', createBase32Hash(key)), { recursive: true })
  }

  readdirSpy.mockRestore()
  statSpy.mockRestore()
  rmSpy.mockRestore()
})

test('cleanExpiredCache does nothing if dlxCacheMaxAge is Infinity', async () => {
  prepareEmpty()

  const cacheDir = path.resolve('cache')
  const dlxCacheMaxAge = Infinity
  const now = new Date()

  const timeTable = {
    foo: 12,
    bar: 1,
    baz: -20,
  }

  for (const [key, minuteDelta] of Object.entries(timeTable)) {
    const dirName = createBase32Hash(key)
    fs.mkdirSync(path.join(cacheDir, 'dlx', dirName, 'node_modules', '.pnpm'), { recursive: true })
    fs.mkdirSync(path.join(cacheDir, 'dlx', dirName, 'node_modules', '.bin'), { recursive: true })
    fs.writeFileSync(path.join(cacheDir, 'dlx', dirName, 'node_modules', '.modules.yaml'), '')
    fs.writeFileSync(path.join(cacheDir, 'dlx', dirName, 'package.json'), '')
    fs.writeFileSync(path.join(cacheDir, 'dlx', dirName, 'pnpm-lock.yaml'), '')
    const newDate = new Date(now.getTime() + minuteDelta * 60_000)
    fs.utimesSync(path.join(cacheDir, 'dlx', dirName), newDate, newDate)
  }

  const readdirSpy = jest.spyOn(fs.promises, 'readdir')
  const statSpy = jest.spyOn(fs.promises, 'stat')
  const rmSpy = jest.spyOn(fs.promises, 'rm')

  await cleanExpiredCache({
    cacheDir,
    dlxCacheMaxAge,
    now,
  })

  expect(
    fs.readdirSync(path.join(cacheDir, 'dlx'))
      .sort()
  ).toStrictEqual(
    ['foo', 'bar', 'baz']
      .map(createBase32Hash)
      .sort()
  )

  expect(readdirSpy).not.toHaveBeenCalled()
  expect(statSpy).not.toHaveBeenCalled()
  expect(rmSpy).not.toHaveBeenCalled()

  readdirSpy.mockRestore()
  statSpy.mockRestore()
  rmSpy.mockRestore()
})
