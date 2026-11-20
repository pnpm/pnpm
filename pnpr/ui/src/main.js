import { createElement } from 'react'
import { createRoot } from 'react-dom/client'
import { BrowserRouter } from 'react-router-dom'
import { PnprWeb } from '@pnpm/pnpr.apps.pnpr-web'
import faviconUrl from '@pnpm/pnpr.apps.pnpr-web/favicon.svg'

// pnpr serves the UI at `<server URL>/-/ui/`, and the server URL may carry a
// path prefix.
const UI_PATH = '/-/ui'
const uiPathStart = window.location.pathname.indexOf(`${UI_PATH}/`)
const serverPath = uiPathStart === -1 ? '' : window.location.pathname.slice(0, uiPathStart)

const favicon = document.createElement('link')
favicon.rel = 'icon'
favicon.type = 'image/svg+xml'
favicon.href = faviconUrl
document.head.append(favicon)

createRoot(document.getElementById('root')).render(
  createElement(
    BrowserRouter,
    { basename: `${serverPath}${UI_PATH}` },
    createElement(PnprWeb, { baseUrl: `${window.location.origin}${serverPath}` })
  )
)
