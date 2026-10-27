import * as React from "react"
import { UserPlusIcon, Trash2Icon } from "lucide-react"
import { toast } from "sonner"

import { Avatar, AvatarFallback } from "@/components/ui/avatar"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
} from "@/components/ui/empty"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupInput,
} from "@/components/ui/input-group"
import { ScrollArea } from "@/components/ui/scroll-area"
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
import {
  enrollSpeaker,
  forgetSpeaker,
  initials,
  speakerLabel,
  type Speaker,
} from "@/lib/api"

type SpeakersPaneProps = {
  speakers: Speaker[]
  galleryPath: string
  lastSpeaker: number | null
  lastName: string | null
  canEnroll: boolean
  onChanged: () => void
}

export function SpeakersPane({
  speakers,
  galleryPath,
  lastSpeaker,
  lastName,
  canEnroll,
  onChanged,
}: SpeakersPaneProps) {
  const [name, setName] = React.useState("")
  const [pending, setPending] = React.useState(false)
  const [forgetName, setForgetName] = React.useState<string | null>(null)

  async function onEnroll() {
    const trimmed = name.trim()
    if (!trimmed) {
      toast.error("Give this speaker a name first.")
      return
    }
    setPending(true)
    try {
      const result = await enrollSpeaker(trimmed, lastSpeaker ?? undefined)
      toast.success(
        `Saved ${result.name} as ${speakerLabel(result.speaker, result.name)}.`
      )
      setName("")
      onChanged()
    } catch (error) {
      toast.error(String(error))
    } finally {
      setPending(false)
    }
  }

  async function onForget() {
    if (!forgetName) {
      return
    }
    setPending(true)
    try {
      await forgetSpeaker(forgetName)
      toast.success(`Forgot ${forgetName}.`)
      setForgetName(null)
      onChanged()
    } catch (error) {
      toast.error(String(error))
    } finally {
      setPending(false)
    }
  }

  return (
    <div className="flex h-full flex-col border-l">
      <div className="flex flex-col gap-2 px-2 py-2">
        <div className="flex items-center gap-2">
          <p className="min-w-0 flex-1 truncate text-[13px] font-medium">
            Speakers
          </p>
          {lastSpeaker !== null ? (
            <Badge variant="outline">
              {speakerLabel(lastSpeaker, lastName)}
            </Badge>
          ) : null}
        </div>
        <FieldGroup className="gap-2">
          <Field>
            <FieldLabel htmlFor="enroll-name" className="sr-only">
              Name current speaker
            </FieldLabel>
            <InputGroup>
              <InputGroupInput
                id="enroll-name"
                placeholder={
                  lastSpeaker === null
                    ? "Name speaker"
                    : speakerLabel(lastSpeaker, lastName)
                }
                value={name}
                onChange={(event) => setName(event.target.value)}
                disabled={!canEnroll || pending}
                title={galleryPath}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault()
                    void onEnroll()
                  }
                }}
              />
              <InputGroupAddon align="inline-end">
                <InputGroupButton
                  onClick={() => void onEnroll()}
                  disabled={!canEnroll || pending}
                >
                  <UserPlusIcon data-icon="inline-start" />
                  Save
                </InputGroupButton>
              </InputGroupAddon>
            </InputGroup>
          </Field>
        </FieldGroup>
      </div>
      <ScrollArea className="min-h-0 flex-1">
        {speakers.length === 0 ? (
          <Empty className="p-3">
            <EmptyHeader>
              <EmptyTitle>No voice prints</EmptyTitle>
              <EmptyDescription>
                Name the person talking while audio is playing.
              </EmptyDescription>
            </EmptyHeader>
          </Empty>
        ) : (
          <ul className="flex flex-col px-1 pb-1">
            {speakers.map((speaker) => (
              <li
                key={speaker.name}
                className="flex items-center gap-2 rounded-md px-1.5 py-1 hover:bg-muted/50"
              >
                <Avatar size="sm">
                  <AvatarFallback>{initials(speaker.name)}</AvatarFallback>
                </Avatar>
                <div className="min-w-0 flex-1">
                  <p className="truncate text-[13px] font-medium">
                    {speaker.name}
                  </p>
                  <p className="text-[11px] text-muted-foreground">
                    {speaker.utterances}{" "}
                    {speaker.utterances === 1 ? "clip" : "clips"}
                  </p>
                </div>
                <Button
                  variant="ghost"
                  size="icon-xs"
                  aria-label={`Forget ${speaker.name}`}
                  onClick={() => setForgetName(speaker.name)}
                >
                  <Trash2Icon />
                </Button>
              </li>
            ))}
          </ul>
        )}
      </ScrollArea>
      <AlertDialog
        open={forgetName !== null}
        onOpenChange={(open) => {
          if (!open) {
            setForgetName(null)
          }
        }}
      >
        <AlertDialogContent size="sm">
          <AlertDialogHeader>
            <AlertDialogTitle>Forget {forgetName}?</AlertDialogTitle>
            <AlertDialogDescription>
              Later sessions will show a numbered speaker until you enroll
              again.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel size="sm">Cancel</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              size="sm"
              onClick={() => void onForget()}
            >
              Forget
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
