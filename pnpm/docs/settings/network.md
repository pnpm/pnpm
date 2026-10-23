---
id: network
title: "Network & Request Settings"
sidebar_label: "Network & requests"
---

## Network settings

### httpsProxy

* Default: **null**
* Type: **url**

A proxy to use for outgoing HTTPS requests. If `httpsProxy` is unset, pnpm uses
the legacy `proxy` setting, then the `HTTPS_PROXY` or `https_proxy` environment
variable.

If your proxy URL contains a username and password, make sure to URL-encode them.
For instance:

```yaml title="pnpm-workspace.yaml"
httpsProxy: "https://use%21r:pas%2As@my.proxy:1234/foo"
```

Do not encode the colon (`:`) between the username and password.

An empty value reads as unset. If neither `httpsProxy` nor `proxy` is configured, a shell that exports `HTTPS_PROXY=` disables the proxy. A `false` or `null` value in `pnpm-workspace.yaml` or the `.npmrc` also reads as unset, so pnpm still falls back to `proxy` and the environment. To turn proxying off, set `proxy` to `false`. On the command line, `false` and `null` are ordinary host names, since a flag carries its value verbatim.

### httpProxy

* Default: **null**
* Type: **url**

A proxy to use for outgoing HTTP requests. If `httpProxy` is unset, pnpm uses
the proxy resolved for [`httpsProxy`](#httpsproxy), then the `HTTP_PROXY` or
`http_proxy` environment variable, then `PROXY` or `proxy`.

Empty, `false`, and `null` values behave as described for [`httpsProxy`](#httpsproxy).

The `.npmrc` also supports npm's legacy `proxy` setting, which is used as the fallback for both `https-proxy` and `http-proxy`. An empty `proxy=` reads as unset, so it does not suppress the `HTTPS_PROXY` environment variable. `proxy=false` (or `proxy: false` in `pnpm-workspace.yaml`) turns proxying off.

### noProxy

* Default: **null**
* Type: **String**

A comma-separated string of domain extensions that a proxy should not be used for.
If `noProxy` is unset, pnpm uses `noproxy`, then the `NO_PROXY` or `no_proxy`
environment variable.

An entry may be written with or without a leading dot: both `npmjs.org` and `.npmjs.org` bypass the proxy for the domain itself and for its subdomains.

Empty, `false`, and `null` values behave as described for [`httpsProxy`](#httpsproxy).

### localAddress

* Default: **undefined**
* Type: **IP Address**

The IP address of the local interface to use when making connections to the npm
registry.

### maxsockets

* Default: **unset**
* Type: **Number**

The maximum number of connections to use per origin (protocol/host/port combination).
When unset, connections through a proxy are limited to 50 per origin, and direct
connections are bounded only by [`networkConcurrency`](#networkconcurrency). Set `0` to remove the limit for proxied connections too.

`maxSockets` is accepted as an alias. Both spellings are read from
`pnpm-workspace.yaml`, the global configuration file, the environment, and the
command line, in that increasing order of precedence, so a value passed on the
command line wins even when the two sides spelled the setting differently.

### strictSsl

* Default: **true**
* Type: **Boolean**

Whether or not to do SSL key validation when making requests to the registry via
HTTPS.

### userAgent

* Default: `pnpm/<version> npm/? node/? <platform> <arch>`
* Type: **String**

The value of the `User-Agent` header that pnpm sends with every request to the registry.

## Request settings

### gitShallowHosts

* Default: **['github.com', 'gist.github.com', 'gitlab.com', 'bitbucket.com', 'bitbucket.org']**
* Type: **string[]**

When fetching dependencies that are Git repositories, if the host is listed in this setting, pnpm will use shallow cloning to fetch only the needed commit, not all the history.

### networkConcurrency

* Default: **auto (workers × 3 clamped to 64-96)**
* Type: **Number**

Controls the maximum number of HTTP(S) requests to process simultaneously.

pnpm selects a value between 64 and 96 based on the number of workers, which is the available parallelism minus one (networkConcurrency = clamp(workers × 3, 64, 96)). Set this value explicitly to override the automatic scaling.

If a request times out while other requests to the same host are still in flight, pnpm lowers the limit for that host to one request for the rest of the command. Requests that are already running finish first. Requests to other hosts keep the full limit. Requests sent through the same proxy count as one host.

To limit requests to one registry, set [`networkConcurrency`](../registries.md#networkconcurrency) on its entry in the `registries` setting.

### fetchRetries

* Default: **2**
* Type: **Number**

How many times to retry if pnpm fails to fetch from the registry.

### fetchRetryFactor

* Default: **10**
* Type: **Number**

The exponential factor for retry backoff.

### fetchRetryMintimeout

* Default: **10000 (10 seconds)**
* Type: **Number**

The lower bound (in milliseconds) of the retry exponential backoff.

### fetchRetryMaxtimeout

* Default: **60000 (1 minute)**
* Type: **Number**

The upper bound (in milliseconds) of the retry exponential backoff.

### fetchTimeout

* Default: **60000 (1 minute)**
* Type: **Number**

How long an HTTP request may go without making progress. The clock is reset on every chunk pnpm receives, so a large tarball on a slow connection is not cut off while data is still arriving; a stalled request is.

Before v12.4.0, this bounded the whole request, which meant the value had to be large enough to download the largest package over the slowest connection you cared about.

### fetchWarnTimeoutMs

* Default: **10000 ms (10 seconds)**
* Type: **Number**

A warning message is displayed if a metadata request to the registry takes longer than the specified threshold (in milliseconds).

### fetchMinSpeedKiBps

* Default: **50 KiB/s**
* Type: **Number**

A warning message is displayed if the download speed of a tarball from the registry falls below the specified threshold (in KiB/s).

### maxTarballUnpackedSize

Added in: v12.15.0

* Default: **4294967296 (4 GiB)**
* Type: **Number**

The most bytes the files of one package may unpack to. A package whose files total more fails to install. A package from a [tar registry](../registries.md#tar-registries) is also held to its `dist.unpackedSize`.

pnpm checks this as it decodes a tarball, before it can verify the tarball's hash, so a hostile registry cannot make it write more than this much.
