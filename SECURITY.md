# Security policy

delune holds Navidrome and Soulseek credentials, writes into your music library,
and accepts connections from the Soulseek network. Please report vulnerabilities
privately.

## Reporting

Use GitHub's [private vulnerability reporting](https://github.com/PndaMan/delune/security/advisories/new).
Include what's affected, how to reproduce it, and the impact you expect. You'll get
an acknowledgement within a week.

Please don't open public issues for security problems.

## Scope

In scope: the delune server and API, the web UI, the TUI, the Soulseek client
(including handling of messages from untrusted peers), and file handling during
import.

Areas of particular interest:
- Path traversal or writes outside the library and staging folders
- Resource exhaustion from malicious peer messages
- Authentication or authorisation bypass between users
- Credential exposure in logs, API responses or the web UI

## Supported versions

Before 1.0, only the latest release gets security fixes.
