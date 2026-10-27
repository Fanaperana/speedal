import { AudioLinesIcon, FileAudioIcon } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
} from "@/components/ui/empty"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Badge } from "@/components/ui/badge"
import { Avatar, AvatarFallback } from "@/components/ui/avatar"
import { cn } from "@/lib/utils"
import {
  formatTime,
  initials,
  speakerLabel,
  type Turn,
} from "@/lib/api"

type TranscriptPaneProps = {
  turns: Turn[]
  live: Turn | null
  listening: boolean
  busy: boolean
  hint?: string
  onListen: () => void
  onOpenFile: () => void
}

export function TranscriptPane({
  turns,
  live,
  listening,
  busy,
  hint,
  onListen,
  onOpenFile,
}: TranscriptPaneProps) {
  const empty = turns.length === 0 && live === null

  if (empty) {
    return (
      <div className="flex h-full items-start justify-center p-6">
        <Empty className="max-w-sm p-0">
          <EmptyHeader>
            <EmptyTitle>No transcript</EmptyTitle>
            <EmptyDescription>
              {hint ??
                "Listen to this computer’s speakers, or open a WAV. Models run locally."}
            </EmptyDescription>
          </EmptyHeader>
          <EmptyContent className="flex-row justify-center">
            <Button size="sm" onClick={onListen} disabled={busy}>
              <AudioLinesIcon data-icon="inline-start" />
              Listen
            </Button>
            <Button
              size="sm"
              variant="outline"
              onClick={onOpenFile}
              disabled={busy}
            >
              <FileAudioIcon data-icon="inline-start" />
              Open WAV
            </Button>
          </EmptyContent>
        </Empty>
      </div>
    )
  }

  return (
    <ScrollArea className="h-full">
      <div className="flex flex-col px-2 py-1">
        {turns.map((turn, index) => (
          <TurnRow
            key={`${turn.speaker}-${turn.start}-${index}`}
            turn={turn}
          />
        ))}
        {live ? <TurnRow turn={live} live={listening} /> : null}
      </div>
    </ScrollArea>
  )
}

function TurnRow({ turn, live = false }: { turn: Turn; live?: boolean }) {
  const label = speakerLabel(turn.speaker, turn.name)

  return (
    <article
      className={cn(
        "flex gap-2 rounded-md px-2 py-1.5 hover:bg-muted/50",
        live && "bg-muted/40"
      )}
    >
      <Avatar size="sm" className="mt-0.5">
        <AvatarFallback>{initials(label)}</AvatarFallback>
      </Avatar>
      <div className="min-w-0 flex-1">
        <div className="flex h-5 items-center gap-1.5">
          <p className="truncate text-[13px] font-medium">{label}</p>
          <p className="shrink-0 text-[11px] text-muted-foreground">
            {formatTime(turn.start)}–{formatTime(turn.end)}
          </p>
          {live ? <Badge variant="secondary">live</Badge> : null}
        </div>
        <p className="text-[13px] leading-snug text-pretty">{turn.text}</p>
      </div>
    </article>
  )
}
