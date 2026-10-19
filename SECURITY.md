# Security Policy

## Reporting a vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

Use [GitHub private vulnerability reporting](https://github.com/Liwyd/termru/security/advisories/new)
for this repository. Include:

* what is affected (component, version/commit, platform);
* steps to reproduce;
* impact assessment if you have one (e.g. remote code execution, secret leak);
* any suggested fix or mitigation.

You will get an acknowledgement within a few business days and a status update
within 14 days. We will coordinate disclosure timing with you.

## Scope

In scope:

* the Termru desktop application and its SSH stack;
* the local SQLite storage layer (secret handling, file permissions);
* the cloud sync API and containers (once released).

Out of scope:

* vulnerabilities in third-party dependencies with no Termru-specific
  aggravating factor (report those upstream; we track advisories via
  `cargo deny` + Dependabot);
* issues requiring a compromised OS or physical access as the primary vector
  without a Termru privilege boundary being crossed.

## Supported versions

During pre-1.0 development only the latest `main` commit is supported.
After 1.0, security fixes ship on the latest minor release line.
