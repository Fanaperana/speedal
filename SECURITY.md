# Security Policy

## Supported versions

| Version | Supported |
| --- | --- |
| 0.1.x | Yes |

## Reporting a vulnerability

Please **do not** open a public issue for security reports.

Use GitHub's private reporting on this repository:

**Security → Report a vulnerability**

or go to
[https://github.com/Fanaperana/speedal/security/advisories/new](https://github.com/Fanaperana/speedal/security/advisories/new)

Include:

- What you found and how to reproduce it
- Affected command or crate (`speedal-asr`, capture, gallery, …)
- Impact (model cache paths, local files, unexpected network, …)

You should hear back within 7 days. If the report is accepted we will credit
you (unless you ask otherwise) and ship a fix on the `0.1.x` line.

speedal runs models on-device. Treat enrolled `speakers.json` centroids as
sensitive: they are voice prints stored on disk.
