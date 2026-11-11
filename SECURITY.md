# Security policy

## Reporting a vulnerability

Please report privately; do not open a public issue.

Use GitHub's **Security → Report a vulnerability** (private advisory) on this
repository. Include the version (`kempt --version`), your OS, and steps to
reproduce. We aim to acknowledge within 7 days and to ship a fix or mitigation
for confirmed issues within 90 days, crediting you if you wish.

## Supported versions

Only the latest release receives security fixes.

## Security model

- `kempt gui` listens on **127.0.0.1 only**, on a random port.
- Every API request needs a per-run random token (`X-Token`) and a loopback
  `Host` header (blocks DNS-rebinding). No CORS headers are sent.
- A strict Content-Security-Policy is set; the UI loads no external resources.
- Files are only read/written when you open/save them; settings and notes live in
  your user config directory (`instance.json` is mode 0600 on Unix).
- The only network access is the **opt-in** update check, done with `curl` to
  `api.github.com` for the configured repository.
- Packages for Windows and macOS are not code-signed; verify downloads with
  `SHA256SUMS`.

Out of scope: attacks needing prior local code execution as the same user.
