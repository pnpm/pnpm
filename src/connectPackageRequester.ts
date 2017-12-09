import {
  PackageResponse,
  RequestPackageFunction,
  RequestPackageOptions,
  Resolution,
  WantedDependency,
} from '@pnpm/package-requester'
import JsonSocket = require('json-socket')
import net = require('net')
import uuid = require('uuid')

export default function (
  opts: {
    port: number,
    hostname?: string,
  },
): Promise<RequestPackageFunction> {
  const socket = new JsonSocket(new net.Socket());
  socket.connect(opts.port, opts.hostname || '127.0.0.1')

  return new Promise((resolve, reject) => {
    socket.on('connect', () => {
      const waiters = createWaiters()

      socket.on('message', (message) => {
        waiters.resolve(message.action, message.body)
      })

      const result = requestPackage.bind(null, socket, waiters)
      result['end'] = () => socket.end() // tslint:disable-line
      resolve(result)
    })
  })
}

function createWaiters () {
  const waiters = {}
  return {
    add (id: string) {
      waiters[id] = deffered()
      return waiters[id].promise
    },
    resolve (id: string, obj: object) {
      if (waiters[id]) {
        waiters[id].resolve(obj)
      }
    },
  }
}

// tslint:disable-next-line
function noop () {}

function deffered<T> (): {
  promise: Promise<T>,
  resolve: (v: T) => void,
  reject: (err: Error) => void,
} {
  let pResolve: (v: T) => void = noop
  let pReject: (err: Error) => void = noop
  const promise = new Promise<T>((resolve, reject) => {
    pResolve = resolve
    pReject = reject
  })
  return {
    promise,
    reject: pReject,
    resolve: pResolve,
  }
}

function requestPackage (
  socket: JsonSocket,
  waiters: object,
  wantedDependency: WantedDependency,
  options: RequestPackageOptions,
): Promise<PackageResponse> {
  const msgId = uuid.v4()

  const fetchingManifest = waiters['add'](`manifestResponse:${msgId}`) // tslint:disable-line
  const fetchingFiles = waiters['add'](`packageFilesResponse:${msgId}`) // tslint:disable-line
  const response = waiters['add'](`packageResponse:${msgId}`) // tslint:disable-line
    .then((packageResponse: object) => {
      return Object.assign(packageResponse, {
        fetchingFiles,
        fetchingManifest,
        finishing: Promise.all([fetchingManifest, fetchingFiles]).then(() => undefined),
      })
    })

  socket.sendMessage({
    msgId,
    options,
    wantedDependency,
  }, (err) => err && console.error(err))

  return response
}
