import { invoke } from "@tauri-apps/api/core"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"
import { open } from "@tauri-apps/plugin-dialog"

export type Profile = "offline" | "low" | "very-low" | "ultra"

export type AppStatus =
  | "idle"
  | "loading"
  | "ready"
  | "listening"
  | "transcribing"
  | "error"

export type Speaker = {
  name: string
  utterances: number
}

export type Turn = {
  start: number
  end: number
  speaker: number
  name: string | null
  text: string
}

export type AppInfo = {
  gallery_path: string
  permission_hint: string
  models_ready: boolean
  profile: string
  asr_chunk_ms: number | null
  diar_latency: number | null
  last_speaker: number | null
}

export type EnrollResult = {
  speaker: number
  name: string
  utterances: number
}

export type UiEvent =
  | { type: "status"; state: AppStatus; detail: string | null }
  | {
      type: "ready"
      asr_chunk_ms: number
      diar_latency: number
      profile: string
      gallery_path: string
      permission_hint: string
    }
  | {
      type: "delta"
      speaker: number
      name: string | null
      start: number
      end: number
      text: string
    }
  | TurnEvent
  | { type: "speakers"; speakers: Speaker[] }
  | { type: "rms"; value: number }
  | { type: "error"; message: string }
  | { type: "cleared" }

type TurnEvent = {
  type: "turn"
  start: number
  end: number
  speaker: number
  name: string | null
  text: string
}

export function speakerLabel(speaker: number, name: string | null | undefined) {
  return name?.trim() ? name : `speaker_${speaker}`
}

export function formatTime(seconds: number) {
  const clamped = Math.max(0, seconds)
  const minutes = Math.floor(clamped / 60)
  const rest = clamped - minutes * 60
  return `${minutes}:${rest.toFixed(1).padStart(4, "0")}`
}

export function initials(label: string) {
  const parts = label.trim().split(/\s+/).filter(Boolean)
  if (parts.length === 0) {
    return "?"
  }
  if (parts.length === 1) {
    return parts[0].slice(0, 2).toUpperCase()
  }
  return `${parts[0][0] ?? ""}${parts[1][0] ?? ""}`.toUpperCase()
}

export async function getAppInfo() {
  return invoke<AppInfo>("app_info")
}

export async function listSpeakers() {
  return invoke<Speaker[]>("list_speakers")
}

export async function forgetSpeaker(name: string) {
  return invoke<Speaker>("forget_speaker", { name })
}

export async function startListen(profile: Profile) {
  return invoke<void>("start_listen", { profile })
}

export async function stopListen() {
  return invoke<void>("stop_listen")
}

export async function transcribeFile(path: string) {
  return invoke<Turn[]>("transcribe_file", { path })
}

export async function enrollSpeaker(name: string, speaker?: number) {
  return invoke<EnrollResult>("enroll_speaker", {
    name,
    speaker: speaker ?? null,
  })
}

export async function clearTranscript() {
  return invoke<void>("clear_transcript")
}

export async function pickWav() {
  return open({
    multiple: false,
    directory: false,
    filters: [{ name: "WAV", extensions: ["wav"] }],
  })
}

export function subscribe(onEvent: (event: UiEvent) => void) {
  return listen<UiEvent>("speedal", (event) => {
    onEvent(event.payload)
  }) as Promise<UnlistenFn>
}
