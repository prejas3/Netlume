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

Packet capture uses [WinDivert 2.2.2](https://github.com/basil00/WinDivert) (LGPL-3.0, loaded dynamically, not in git): run `src-tauri/windivert.ps1` to fetch the signed x64 `WinDivert.dll` and `WinDivert64.sys` into `src-tauri/windivert/`, then put that folder on `PATH`. Microsoft Defender may flag WinDivert as unwanted software.

The desktop app requests administrator privileges because the finished limiter will manage system network traffic.
Release builds request elevation; engine development that needs packet access must run from an elevated terminal. Mock-data `tauri:dev` runs unelevated for UI work.
