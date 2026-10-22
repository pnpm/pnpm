---
id: tls-and-reverse-proxy
title: Reverse proxy and TLS
---

pnpr is designed as an internal HTTP application service. It deliberately does not implement built-in TLS termination listeners or automated certificate management (ACME). In production, pnpr runs behind a TLS-terminating reverse proxy (such as Nginx, Caddy, or Traefik) or a cloud load balancer (such as AWS Application Load Balancer, Google Cloud HTTPS Load Balancing, or Cloudflare).

:::note

While `rustls` is present in pnpr's dependency tree, it is used strictly as an outbound TLS client for connecting to upstream registries and remote LibSQL databases. It does not provide inbound HTTPS termination for the server.

:::

## Why use a reverse proxy?

Fronting pnpr with a reverse proxy or load balancer provides:

- **TLS termination**: Automates certificate issuance and renewal via Let's Encrypt or your organization's PKI, and negotiates modern HTTP protocols (HTTP/2 and HTTP/3).
- **Connection buffering**: Buffers slow client connections and streams large package tarballs and container image layers without exhausting application worker threads.
- **DDoS and edge protection**: Applies connection limits, request filtering, and web application firewall (WAF) policies before traffic reaches the registry.
- **Zero-downtime rolling updates**: Cooperates with pnpr's graceful shutdown signals and readiness probes to route traffic between instances during restarts or version upgrades.

## Configuring pnpr for reverse proxies

When running behind a reverse proxy, start pnpr with the public URL and set
the trusted proxies in your `pnpr.yaml`:

```sh
pnpr -c ./pnpr.yaml --public-url https://registry.example.com
```

```yaml title="pnpr.yaml"
# Reverse proxy IPs or CIDR blocks trusted to forward client IPs
trustedProxies:
  - loopback
  - 10.0.0.0/8
  - 172.16.0.0/12

# Header to inspect for the client IP (defaults to x-forwarded-for)
trustedProxyHeader: x-forwarded-for
```

### Public URL

`--public-url` (or `PNPR_PUBLIC_URL`) tells pnpr how clients reach the registry from the outside. When clients fetch packuments, pnpr rewrites tarball and attestation URLs to use this base URL instead of its internal listen address (`http://127.0.0.1:7677`).

### trustedProxies

By default, pnpr ignores forwarding headers such as `X-Forwarded-For` and attributes requests to the direct socket peer. When running behind a reverse proxy or a chain of proxies, configure `trustedProxies` with the IP addresses or CIDR blocks of every proxy hop between the client and pnpr.

When `trustedProxies` is set with `trustedProxyHeader: x-forwarded-for` (the default), pnpr parses the header from right to left, skipping trusted proxy hops and stopping at the first untrusted IP address. If an intermediate proxy in the forwarding chain is omitted from `trustedProxies`, traversal stops early at that proxy's IP instead of reaching the true originating client IP. When `trustedProxyHeader: x-real-ip` is configured, pnpr reads the single IP address provided in `X-Real-IP` by the immediate trusted proxy.

The local reverse-proxy recipes (such as Nginx and Caddy) connect over `loopback`. The AWS ALB recipe uses the VPC CIDR block for direct ALB-to-pnpr traffic. If your architecture routes traffic through multiple proxy layers (such as a CDN or edge load balancer followed by an internal ingress controller), include all intermediate proxy networks in `trustedProxies`.

The client IP determined by this traversal is used for:
- [Login rate limiting](configuration.md#security-limits-and-protections)
- Structured audit logs (`pnpr::audit`) and access logs (`pnpr::access`)
- CIDR-restricted bearer tokens

:::warning

Do not set `trustedProxies` to trust all addresses (`0.0.0.0/0`). An overly permissive configuration allows untrusted clients to spoof their IP address by injecting forged `X-Forwarded-For` headers.

:::

## Health and readiness checks

Point the proxy's health probes at `GET /-/ready` for readiness and
`GET /-/live` for liveness. The responses are described in
[HTTP endpoints](endpoints.md#always-available).

## Configuration recipes

### Nginx

A complete Nginx virtual host configuration:

```nginx title="nginx.conf"
upstream pnpr_backend {
    server 127.0.0.1:7677;
    keepalive 32;
}

server {
    listen 80;
    server_name registry.example.com;
    return 301 https://$host$request_uri;
}

server {
    listen 443 ssl http2;
    server_name registry.example.com;

    ssl_certificate /etc/letsencrypt/live/registry.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/registry.example.com/privkey.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers HIGH:!aNULL:!MD5;

    # Allow large tarball and container image uploads (default is 1M)
    client_max_body_size 500M;

    # Disable request body buffering to stream large uploads directly to pnpr
    proxy_request_buffering off;
    proxy_buffering off;

    # Timeouts for large package transfers
    proxy_connect_timeout 60s;
    proxy_read_timeout 300s;
    proxy_send_timeout 300s;

    location / {
        proxy_pass http://pnpr_backend;
        proxy_http_version 1.1;
        proxy_set_header Connection "";

        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

### Caddy

Caddy manages TLS certificates automatically and proxies traffic with minimal configuration:

```caddyfile title="Caddyfile"
registry.example.com {
    # Allow uploads up to 500 MB
    request_body {
        max_size 500MB
    }

    reverse_proxy 127.0.0.1:7677 {
        header_up Host {host}
        header_up X-Real-IP {remote_host}
        header_up X-Forwarded-Proto {scheme}
    }
}
```

### Traefik

A Traefik configuration using Docker Compose labels:

```yaml title="docker-compose.yml"
services:
  pnpr:
    image: ghcr.io/pnpm/pnpr:latest
    environment:
      PNPR_LISTEN: 0.0.0.0:7677
      PNPR_PUBLIC_URL: https://registry.example.com
    labels:
      - "traefik.enable=true"
      - "traefik.http.routers.pnpr.rule=Host(`registry.example.com`)"
      - "traefik.http.routers.pnpr.entrypoints=websecure"
      - "traefik.http.routers.pnpr.tls.certresolver=letsencrypt"
      - "traefik.http.services.pnpr.loadbalancer.server.port=7677"
      - "traefik.http.services.pnpr.loadbalancer.healthcheck.path=/-/ready"
      - "traefik.http.services.pnpr.loadbalancer.healthcheck.interval=10s"
```

### AWS Application Load Balancer (ALB)

When deploying behind an AWS Application Load Balancer:

1. Create a Target Group with target type **IP** or **Instance** on port 7677 using HTTP.
2. Set the health check path to `/-/ready` with an HTTP status code of `200`.
3. Create an HTTPS listener on port 443 with your ACM certificate, forwarding traffic to the Target Group.
4. Ensure the ALB idle timeout is set high enough for large package uploads (for example, 300 seconds).
5. Restrict inbound TCP port 7677 on the pnpr targets to the ALB security group. Do not allow other VPC sources to reach this port directly.
6. In `pnpr.yaml`, add the VPC CIDR block (such as `10.0.0.0/16`) to `trustedProxies`.

## Process lifecycle and configuration reload

pnpr applies configuration changes only on restart. See
[Signals and graceful shutdown](cli.md#signals-and-graceful-shutdown) for how to
roll them out without downtime.
