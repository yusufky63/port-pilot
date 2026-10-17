# Security policy

Port Pilot can end processes on a developer's machine, so security reports get priority.

## Supported versions

Only the latest release receives fixes.

## Reporting a vulnerability

Please do not open a public issue. Use GitHub's private reporting: **Security > Report a vulnerability** on this repository. Include the version, Windows build, steps to reproduce and what an attacker gains. Do not attach full process lists or unmasked command lines; they can contain secrets.

You can expect a first answer within a week. Fixed issues are credited in the release notes unless you ask otherwise.

## What counts

Examples that are in scope:

- Any way to make Port Pilot signal or terminate a process other than the one the user confirmed (PID reuse, a crafted command line, a race between scan and action).
- CTRL+C or CTRL+BREAK reaching a process outside the target's own console group.
- The WebView or a local page causing Port Pilot to run a program, open an arbitrary path or stop a process without the user.
- Secrets in command lines shown, printed or written while masking is on.
- Any network traffic at all.

The design and threat model are in [docs/security.md](docs/security.md).
