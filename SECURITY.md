# Security

United stores messenger sessions locally in `%LOCALAPPDATA%\United\WebData` and does not run a cloud backend. Treat that folder like a password store: anyone who can read your Windows profile can read the saved sessions.

## Reporting a vulnerability

Please do not open a public issue for a vulnerability that could expose sessions, messages, or local data. Use GitHub's **Report a vulnerability** (private security advisory) on this repository instead. Include the United version, Windows version, WebView2 Runtime version, and clear reproduction steps. Do not attach real chat content, cookies, tokens, or personal logs.

## Scope notes

- The app embeds official service websites in WebView2; vulnerabilities in those sites should be reported to the service owner.
- Release builds do not enable remote debugging or DevTools (`AreDevToolsEnabled` is only true when a debugger is attached).
- Custom service URLs are loaded as-is. Only add services you trust; a malicious page can request camera/microphone only for its own origin, and other permissions are denied by the host.
