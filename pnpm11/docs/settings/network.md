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

Since v11.20.0, an empty value reads as unset instead of failing the install with `ERR_PNPM_INVALID_PROXY`. If neither `httpsProxy` nor `proxy` is configured, a shell that exports `HTTPS_PROXY=` disables the proxy. A `false` or `null` value in `pnpm-workspace.yaml` or the `.npmrc` also reads as unset, so pnpm still falls back to `proxy` and the environment. To turn proxying off, set `proxy` to `false`. On the command line, `false` and `null` are ordinary host names, since a flag carries its value verbatim.

### httpProxy

* Default: **null**
* Type: **url**

A proxy to use for outgoing HTTP requests. If `httpProxy` is unset, pnpm uses
the proxy resolved for [`httpsProxy`](#httpsproxy), then the `HTTP_PROXY` or
`http_proxy` environment variable, then `PROXY` or `proxy`.

Empty, `false`, and `null` values behave as described for [`httpsProxy`](#httpsproxy).

The `.npmrc` also supports npm's legacy `proxy` setting, which is used as the fallback for both `https-proxy` and `http-proxy`. Since v11.20.0, an empty `proxy=` reads as unset and therefore no longer suppresses the `HTTPS_PROXY` environment variable, and `proxy=false` (or `proxy: false` in `pnpm-workspace.yaml`) turns proxying off instead of being read as a proxy host named `false` ([#13533](https://github.com/pnpm/pnpm/issues/13533)).

### noProxy

* Default: **null**
* Type: **String**

A comma-separated string of domain extensions that a proxy should not be used for.
If `noProxy` is unset, pnpm uses `noproxy`, then the `NO_PROXY` or `no_proxy`
environment variable.

Empty, `false`, and `null` values behave as described for [`httpsProxy`](#httpsproxy).

### localAddress

* Default: **undefined**
* Type: **IP Address**

The IP address of the local interface to use when making connections to the npm
registry.

### maxsockets

* Default: **networkConcurrency x 3** if [`networkConcurrency`](#networkconcurrency) is set, otherwise **50**
* Type: **Number**

The maximum number of connections to use per origin (protocol/host/port combination).

`maxSockets` is accepted as an alias. Both spellings are read from
`pnpm-workspace.yaml`, the global configuration file, the environment, and the
command line, in that increasing order of precedence, so a value passed on the
command line wins even when the two sides spelled the setting differently.

### strictSsl

* Default: **true**
* Type: **Boolean**

Whether or not to do SSL key validation when making requests to the registry via
HTTPS.

## Request settings

### gitShallowHosts

* Default: **['github.com', 'gist.github.com', 'gitlab.com', 'bitbucket.com', 'bitbucket.org']**
* Type: **string[]**

When fetching dependencies that are Git repositories, if the host is listed in this setting, pnpm will use shallow cloning to fetch only the needed commit, not all the history.

### networkConcurrency

* Default: **auto (workers × 3 clamped to 64-96)**
* Type: **Number**

Controls the maximum number of HTTP(S) requests to process simultaneously.

pnpm automatically selects a value between 64 and 96 based on the number of workers (networkConcurrency = clamp(workers × 3, 64, 96)). Set this value explicitly to override the automatic scaling.

If a request times out while other requests to the same host are still in flight, pnpm lowers the limit for that host to one request for the rest of the command. Requests that are already running finish first. Requests to other hosts keep the full limit. Requests sent through the same proxy count as one host.

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

The maximum amount of time to wait for HTTP requests to connect and complete.
This time should be enough to download the largest package over a reasonable connection.

### fetchWarnTimeoutMs

Added in: v10.18.0

* Default: **10000 ms (10 seconds)**
* Type: **Number**

A warning message is displayed if a metadata request to the registry takes longer than the specified threshold (in milliseconds).

### fetchMinSpeedKiBps

Added in: v10.18.0

* Default: **50 KiB/s**
* Type: **Number**

A warning message is displayed if the download speed of a tarball from the registry falls below the specified threshold (in KiB/s).

### maxTarballUnpackedSize

Added in: v11.28.7

* Default: **4294967296 (4 GiB)**
* Type: **Number**

The most bytes the files of one package may unpack to. A package whose files total more fails to install with the error `ERR_PNPM_TARBALL_TOO_LARGE`.

Raise the setting if you need to install a legitimately large package.
