# Contributing to Kempt

Thanks for helping. Kempt is a small, offline-first tool; changes that keep it
fast, private and dependency-light are the easiest to accept.

## Licence of contributions (inbound = outbound)

Kempt is released under the [MIT licence](LICENSE). By submitting a contribution
you agree it is licensed under the same MIT licence, and you certify the
[Developer Certificate of Origin 1.1](https://developercertificate.org/):
you wrote the change or have the right to submit it under MIT.

Show this by signing off every commit:

```
git commit -s -m "Fix JSON fold on empty arrays"
```

which adds `Signed-off-by: Your Name <you@example.com>`. No separate CLA is
required. Do not submit code copied from projects with incompatible licences
(GPL, AGPL, proprietary).

## Getting started

```
cargo build --release
cargo test --release
python3 tests/test_files.py        # CLI / formatter / viewer
python3 tests/test_ui.py           # browser GUI (needs Playwright + Chromium)
```

The whole UI is one file, `ui/index.html`, embedded into the binary: rebuild
after editing it. See `tests/` for the other suites (each prints PASS/FAIL).

## Pull requests

- One topic per PR; explain the why. Link the issue if there is one.
- Add or update tests for behaviour changes; all suites you touched must pass.
- Keep the app offline: no telemetry, no CDN assets, no new network calls
  (the only one is the opt-in update check).
- Keep new dependencies to a minimum, with permissive licences (MIT, Apache-2.0,
  BSD, ISC, Zlib, Unlicense). Mention why in the PR.
- Security issues: do **not** open a public issue, see [SECURITY.md](SECURITY.md).

## Conduct

Participation is governed by the [Code of Conduct](CODE_OF_CONDUCT.md).
