import * as React from "react"
import {
  AudioLinesIcon,
  FileAudioIcon,
  MoonIcon,
  SquareIcon,
  SunIcon,
  Trash2Icon,
} from "lucide-react"
import { toast } from "sonner"

import { SpeakersPane } from "@/components/speakers-pane"
import { TranscriptPane } from "@/components/transcript-pane"
import { useTheme } from "@/components/theme-provider"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable"
import { Spinner } from "@/components/ui/spinner"
import {
  ToggleGroup,
  ToggleGroupItem,
} from "@/components/ui/toggle-group"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import {
  clearTranscript,
  getAppInfo,
  listSpeakers,
  pickWav,
  removeTurn,
  sameTurn,
  startListen,
  stopListen,
  subscribe,
  transcribeFile,
  type AppStatus,
  type Profile,
  type Speaker,
  type Turn,
} from "@/lib/api"

const PROFILES: { value: Profile; label: string; hint: string }[] = [
  { value: "low", label: "Low", hint: "1.04s buffer · live default" },
  { value: "very-low", label: "Very low", hint: "0.64s buffer" },
  { value: "ultra", label: "Ultra", hint: "0.32s buffer" },
  { value: "offline", label: "Offline", hint: "30.4s buffer · files" },
]

export function App() {
  const { theme, setTheme } = useTheme()
  const [status, setStatus] = React.useState<AppStatus>("idle")
  const [detail, setDetail] = React.useState<string | null>(null)
  const [error, setError] = React.useState<string | null>(null)
  const [desktopReady, setDesktopReady] = React.useState(false)
  const [profile, setProfile] = React.useState<Profile>("low")
  const [turns, setTurns] = React.useState<Turn[]>([])
  const [live, setLive] = React.useState<Turn | null>(null)
  const [speakers, setSpeakers] = React.useState<Speaker[]>([])
  const [galleryPath, setGalleryPath] = React.useState("")
  const [permissionHint, setPermissionHint] = React.useState("")
  const [rms, setRms] = React.useState(0)
  const [asrChunkMs, setAsrChunkMs] = React.useState<number | null>(null)
  const [diarLatency, setDiarLatency] = React.useState<number | null>(null)
  const [busy, setBusy] = React.useState(false)
  const [confirmClear, setConfirmClear] = React.useState(false)
  const liveRef = React.useRef<Turn | null>(null)

  const listening = status === "listening"
  const loading = status === "loading" || status === "transcribing" || busy
  const lastSpeaker = live?.speaker ?? turns.at(-1)?.speaker ?? null
  const lastName = live?.name ?? turns.at(-1)?.name ?? null

  const refreshSpeakers = React.useCallback(async () => {
    try {
      setSpeakers(await listSpeakers())
    } catch (cause) {
      toast.error(String(cause))
    }
  }, [])

  React.useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const info = await getAppInfo()
        if (cancelled) {
          return
        }
        setDesktopReady(true)
        setGalleryPath(info.gallery_path)
        setPermissionHint(info.permission_hint)
        setAsrChunkMs(info.asr_chunk_ms)
        setDiarLatency(info.diar_latency)
        if (
          info.profile === "low" ||
          info.profile === "very-low" ||
          info.profile === "ultra" ||
          info.profile === "offline"
        ) {
          setProfile(info.profile)
        }
        setStatus(info.models_ready ? "ready" : "idle")
        await refreshSpeakers()
      } catch {
        if (!cancelled) {
          setDesktopReady(false)
          setError("Open this window with `pnpm tauri dev` from apps/desktop.")
        }
      }
    })()
    return () => {
      cancelled = true
    }
  }, [refreshSpeakers])

  React.useEffect(() => {
    if (!desktopReady) {
      return
    }
    let unlisten: (() => void) | undefined
    void subscribe((event) => {
      switch (event.type) {
        case "status":
          setStatus(event.state)
          setDetail(event.detail)
          if (event.state !== "error") {
            setError(null)
          }
          break
        case "ready":
          setStatus("ready")
          setGalleryPath(event.gallery_path)
          setPermissionHint(event.permission_hint)
          setAsrChunkMs(event.asr_chunk_ms)
          setDiarLatency(event.diar_latency)
          setDetail(null)
          break
        case "delta": {
          const current = liveRef.current
          const next: Turn =
            current && current.speaker === event.speaker
              ? {
                  ...current,
                  end: event.end,
                  name: event.name,
                  text: `${current.text}${event.text}`,
                }
              : {
                  start: event.start,
                  end: event.end,
                  speaker: event.speaker,
                  name: event.name,
                  text: event.text,
                }
          if (current && current.speaker !== event.speaker) {
            setTurns((turns) => [...turns, current])
          }
          liveRef.current = next
          setLive(next)
          break
        }
        case "turn": {
          const closed: Turn = {
            start: event.start,
            end: event.end,
            speaker: event.speaker,
            name: event.name,
            text: event.text,
          }
          setTurns((current) => {
            const exists = current.some((turn) => sameTurn(turn, closed))
            return exists ? current : [...current, closed]
          })
          if (liveRef.current?.speaker === event.speaker) {
            liveRef.current = null
            setLive(null)
          }
          break
        }
        case "speakers":
          setSpeakers(event.speakers)
          break
        case "rms":
          setRms(event.value)
          break
        case "error":
          setStatus("error")
          setError(event.message)
          setBusy(false)
          toast.error(event.message)
          break
        case "cleared":
          liveRef.current = null
          setTurns([])
          setLive(null)
          setRms(0)
          break
        case "turn_removed":
          setTurns((current) =>
            current.filter((turn) => !sameTurn(turn, event))
          )
          if (liveRef.current && sameTurn(liveRef.current, event)) {
            liveRef.current = null
            setLive(null)
          }
          break
      }
    }).then((fn) => {
      unlisten = fn
    })
    return () => {
      unlisten?.()
    }
  }, [desktopReady])


  async function onListen() {
    setBusy(true)
    setError(null)
    setStatus("loading")
    try {
      await startListen(profile)
    } catch (cause) {
      setStatus("error")
      setError(String(cause))
      toast.error(String(cause))
    } finally {
      setBusy(false)
    }
  }

  async function onStop() {
    setBusy(true)
    try {
      await stopListen()
    } catch (cause) {
      toast.error(String(cause))
    } finally {
      setBusy(false)
    }
  }

  async function onOpenFile() {
    try {
      const selected = await pickWav()
      if (!selected || Array.isArray(selected)) {
        return
      }
      setBusy(true)
      setError(null)
      await transcribeFile(selected)
    } catch (cause) {
      setStatus("error")
      setError(String(cause))
      toast.error(String(cause))
    } finally {
      setBusy(false)
    }
  }

  async function onClear() {
    try {
      await clearTranscript()
      setConfirmClear(false)
    } catch (cause) {
      toast.error(String(cause))
    }
  }

  async function onRemoveTurn(turn: Turn) {
    try {
      await removeTurn(turn.speaker, turn.start)
    } catch (cause) {
      toast.error(String(cause))
    }
  }

  const meter = Math.min(1, Math.max(0, rms * 8))

  return (
    <div className="flex h-svh min-h-0 flex-col overflow-hidden bg-background text-[13px]">
      <header className="flex h-9 shrink-0 items-center gap-1.5 border-b px-2">
        <h1 className="px-1 font-heading text-[13px] font-medium">speedal</h1>
        <StatusBadge status={status} />
        {asrChunkMs !== null && diarLatency !== null ? (
          <p className="hidden min-w-0 truncate text-[11px] text-muted-foreground md:block">
            {asrChunkMs}ms · {diarLatency.toFixed(2)}s
          </p>
        ) : null}
        <div className="ml-auto flex items-center gap-1">
          <div
            className="hidden h-1 w-14 overflow-hidden rounded-full bg-muted sm:block"
            aria-hidden="true"
          >
            <div
              className="h-full bg-primary transition-[width]"
              style={{ width: `${Math.round(meter * 100)}%` }}
            />
          </div>
          <ToggleGroup
            value={[profile]}
            onValueChange={(value) => {
              if (value[0]) {
                setProfile(value[0] as Profile)
              }
            }}
            variant="outline"
            size="sm"
            spacing={0}
            disabled={listening || loading}
            aria-label="Diarization latency"
          >
            {PROFILES.map((item) => (
              <ToggleGroupItem key={item.value} value={item.value} title={item.hint}>
                {item.label}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
          {listening ? (
            <Button
              size="sm"
              variant="destructive"
              onClick={() => void onStop()}
              disabled={busy}
            >
              {busy ? <Spinner data-icon="inline-start" /> : <SquareIcon data-icon="inline-start" />}
              Stop
            </Button>
          ) : (
            <Button
              size="sm"
              onClick={() => void onListen()}
              disabled={loading || !desktopReady}
            >
              {loading ? (
                <Spinner data-icon="inline-start" />
              ) : (
                <AudioLinesIcon data-icon="inline-start" />
              )}
              Listen
            </Button>
          )}
          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  variant="outline"
                  size="icon-xs"
                  disabled={loading || !desktopReady}
                  onClick={() => void onOpenFile()}
                />
              }
            >
              <FileAudioIcon />
              <span className="sr-only">Open WAV</span>
            </TooltipTrigger>
            <TooltipContent>Open WAV</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  variant="ghost"
                  size="icon-xs"
                  disabled={turns.length === 0 && live === null}
                  onClick={() => setConfirmClear(true)}
                />
              }
            >
              <Trash2Icon />
              <span className="sr-only">Clear transcript</span>
            </TooltipTrigger>
            <TooltipContent>Clear all turns</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  variant="ghost"
                  size="icon-xs"
                  onClick={() =>
                    setTheme(
                      theme === "dark"
                        ? "light"
                        : theme === "light"
                          ? "system"
                          : "dark"
                    )
                  }
                />
              }
            >
              {theme === "light" ? <SunIcon /> : <MoonIcon />}
              <span className="sr-only">Cycle theme</span>
            </TooltipTrigger>
            <TooltipContent>Theme: {theme}</TooltipContent>
          </Tooltip>
        </div>
      </header>
      {error || loading || detail ? (
        <div className="shrink-0 border-b px-2 py-1">
          {error ? (
            <Alert variant="destructive">
              <AlertTitle>Error</AlertTitle>
              <AlertDescription className="whitespace-pre-wrap">
                {error}
              </AlertDescription>
            </Alert>
          ) : (
            <p className="flex items-center gap-1.5 text-[12px] text-muted-foreground">
              {loading ? <Spinner /> : null}
              <span className="truncate">
                {status === "loading"
                  ? (detail ?? "Loading models · first run ~3 GB")
                  : status === "transcribing"
                    ? (detail ?? "Aligning speakers")
                    : (detail ?? "Listening")}
              </span>
            </p>
          )}
        </div>
      ) : null}
      <div className="min-h-0 flex-1 overflow-hidden">
        <ResizablePanelGroup
          orientation="horizontal"
          className="h-full min-h-0"
        >
          <ResizablePanel
            defaultSize="78%"
            minSize="50%"
            className="min-h-0 overflow-hidden"
          >
            <div className="h-full min-h-0 overflow-hidden">
              <TranscriptPane
                turns={turns}
                live={live}
                listening={listening}
                busy={loading || !desktopReady}
                hint={permissionHint || undefined}
                onListen={() => void onListen()}
                onOpenFile={() => void onOpenFile()}
                onRemove={(turn) => void onRemoveTurn(turn)}
              />
            </div>
          </ResizablePanel>
          <ResizableHandle />
          <ResizablePanel
            defaultSize="22%"
            minSize="18%"
            className="min-h-0 overflow-hidden"
          >
            <div className="h-full min-h-0 overflow-hidden">
              <SpeakersPane
                speakers={speakers}
                galleryPath={galleryPath}
                lastSpeaker={lastSpeaker}
                lastName={lastName}
                canEnroll={desktopReady && (listening || turns.length > 0)}
                onChanged={() => void refreshSpeakers()}
              />
            </div>
          </ResizablePanel>
        </ResizablePanelGroup>
      </div>
      <AlertDialog open={confirmClear} onOpenChange={setConfirmClear}>
        <AlertDialogContent size="sm">
          <AlertDialogHeader>
            <AlertDialogTitle>Clear all turns?</AlertDialogTitle>
            <AlertDialogDescription>
              This removes the whole transcript. To drop one line, hover it and
              click remove.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel size="sm">Cancel</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              size="sm"
              onClick={() => void onClear()}
            >
              Clear all
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}

function StatusBadge({ status }: { status: AppStatus }) {
  const label =
    status === "listening"
      ? "Listening"
      : status === "loading"
        ? "Loading"
        : status === "transcribing"
          ? "Transcribing"
          : status === "error"
            ? "Error"
            : status === "ready"
              ? "Ready"
              : "Idle"

  return (
    <Badge variant={status === "error" ? "destructive" : "secondary"}>
      {label}
    </Badge>
  )
}

export default App
