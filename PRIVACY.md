# Privacy policy

Spencer Macro Utilities does not collect telemetry, analytics, account
information, macro contents, profile contents, or gameplay data.

SMU makes one automatic HTTPS request per running session to GitHub's public
release API to determine whether an update is available. This request is sent
to:

```text
https://api.github.com/repos/Spencer0187/Spencer-Macro-Utilities/releases/latest
```

The request contains normal network metadata such as the user's IP address,
TLS connection information, and SMU's HTTP user-agent. GitHub processes that
request under [GitHub's Privacy Statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement).
SMU does not add a device identifier, profile data, macro data, or telemetry.

If the user confirms an automatic update, SMU downloads the selected package
from the official Spencer Macro Utilities release path on `github.com`.
Opening the release page, documentation, sponsor page, or another external
link is also an explicit user action handled by the system's default browser.

The Linux privileged network helper communicates only with the local SMU
process that launched it. It does not send data over the network.

SMU does not operate a server that receives user data. Questions about this
policy can be raised through the repository's public issue tracker:

https://github.com/Spencer0187/Spencer-Macro-Utilities/issues
