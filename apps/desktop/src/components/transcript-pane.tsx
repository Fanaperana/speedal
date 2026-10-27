import { AudioLinesIcon, FileAudioIcon, XIcon } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
} from "@/components/ui/empty"
import { Badge } from "@/components/ui/badge"
import { Avatar, AvatarFallback } from "@/components/ui/avatar"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
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
  onRemove: (turn: Turn) => void
}

export function TranscriptPane({
  turns,
  live,
  listening,
  busy,
  hint,
  onListen,
  onOpenFile,
  onRemove,
}: TranscriptPaneProps) {
  const empty = turns.length === 0 && live === null

  if (empty) {
    return (
      <div className="flex h-full min-h-0 items-start justify-center overflow-hidden p-6">
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
    <div className="h-full min-h-0 overflow-y-auto overscroll-contain">
      <div className="flex flex-col px-2 py-1">
        {turns.map((turn, index) => (
          <TurnRow
            key={`${turn.speaker}-${turn.start}-${index}`}
            turn={turn}
            onRemove={() => onRemove(turn)}
          />
        ))}
        {live ? (
          <TurnRow
            turn={live}
            live={listening}
            onRemove={() => onRemove(live)}
          />
        ) : null}
      </div>
    </div>
  )
}

function TurnRow({
  turn,
  live = false,
  onRemove,
}: {
  turn: Turn
  live?: boolean
  onRemove?: () => void
}) {
  const label = speakerLabel(turn.speaker, turn.name)

  return (
    <article
      className={cn(
        "group flex gap-2 rounded-md px-2 py-1.5 hover:bg-muted/50",
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
      {onRemove ? (
        <Tooltip>
          <TooltipTrigger
            render={
              <Button
                variant="ghost"
                size="icon-xs"
                className="mt-0.5 opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 focus-visible:opacity-100"
                aria-label={`Remove ${label}`}
                onClick={onRemove}
              />
            }
          >
            <XIcon />
            <span className="sr-only">Remove this turn</span>
          </TooltipTrigger>
          <TooltipContent>Remove this turn</TooltipContent>
        </Tooltip>
      ) : null}
    </article>
  )
}
