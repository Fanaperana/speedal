# speedal desktop

Tauri v2 app with a React + TypeScript frontend and shadcn/ui. It talks to the same `speedal-session` crate as the CLI.

```bash
pnpm install
pnpm tauri dev
```

First Listen / Open WAV loads the ONNX models (about 3 GB on a cold cache). After that, startup is load-from-disk.

macOS 14.4+ needs the system-audio permission prompt. The usage string lives in `src-tauri/Info.plist`.
