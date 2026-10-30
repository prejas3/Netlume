# Netlume

Netlume is an open-source network monitor and per-application bandwidth limiter for Windows 10 and 11.

## Status

Early MVP scaffold. The app currently emits mock application statistics once per second; WinDivert monitoring and shaping will replace the mock backend.

## Development

Prerequisites: Node.js, Rust, the Tauri 2 Windows prerequisites, and WebView2.

```powershell
npm install
npm run tauri:dev
```

The desktop app requests administrator privileges because the finished limiter will manage system network traffic.
