# Security policy

## Supported versions

Security fixes are applied to the latest release on the default branch. Older
released versions are not supported unless a maintainer explicitly states
otherwise.

## Reporting a vulnerability

Please **do not** open a public issue for a suspected vulnerability. Report it
privately to [the repository owner](https://github.com/RephlexZero) with:

- a description of the impact and affected versions;
- minimal steps or a proof of concept to reproduce it;
- any suggested mitigation, if known; and
- a way to contact you securely for follow-up.

Please avoid including real vehicle logs, precise location data, API tokens, or
other sensitive telemetry in the report. Redacted or synthetic VBO fixtures are
preferred.

We aim to acknowledge reports within 7 days and will keep you informed while
we validate, remediate, and coordinate disclosure. Please allow time for a fix
before sharing details publicly.

## Scope

Reports are especially welcome for denial-of-service risks (including malformed
or oversized VBO input), unsafe parsing behaviour, data disclosure, dependency
vulnerabilities, and release or CI supply-chain issues.
