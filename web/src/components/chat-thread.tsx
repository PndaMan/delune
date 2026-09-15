import { SendHorizontal } from "lucide-react"
import { useEffect, useLayoutEffect, useRef, useState } from "react"

import { type ChatMessage, chatTime } from "@/lib/chat"
import { cn } from "@/lib/utils"

type Props = {
  messages: ChatMessage[]
  /** Our Soulseek name, to tell our own lines apart in rooms. */
  self: string | null
  placeholder: string
  onSend: (text: string) => Promise<unknown>
  disabled?: boolean
  empty: React.ReactNode
}

/**
 * A chat log and a composer. Lines from the same person within a few minutes group
 * under one name, the way people read chat. New lines keep the view at the bottom
 * unless you've scrolled up to read.
 */
export function ChatThread({ messages, self, placeholder, onSend, disabled, empty }: Props) {
  const log = useRef<HTMLDivElement>(null)
  const pinned = useRef(true)

  useLayoutEffect(() => {
    const el = log.current
    if (el && pinned.current) el.scrollTop = el.scrollHeight
  }, [messages.length])

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div
        ref={log}
        onScroll={(e) => {
          const el = e.currentTarget
          pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80
        }}
        className="scrollbar-themed min-h-0 flex-1 overflow-y-auto px-4 py-4 sm:px-6"
        aria-live="polite"
      >
        {messages.length === 0 ? (
          <div className="flex h-full items-center justify-center text-center text-[15px] text-muted-foreground">{empty}</div>
        ) : (
          <ol className="space-y-0.5">
            {messages.map((m, i) => {
              const previous = messages[i - 1]
              const grouped = previous && previous.from === m.from && m.at - previous.at < 5 * 60
              const own = m.outgoing || (!!self && m.from === self)
              return (
                <li key={m.id} className={cn("group grid grid-cols-[1fr_auto] gap-x-3", !grouped && "pt-3")}>
                  {!grouped && (
                    <p className={cn("col-span-2 text-[13.5px] font-semibold", own ? "text-primary" : "text-foreground")}>
                      {own ? "You" : m.from}
                    </p>
                  )}
                  <p className="text-[15px] leading-relaxed break-words whitespace-pre-wrap">{m.text}</p>
                  <time className="pt-1 text-[11.5px] text-muted-foreground/0 group-hover:text-muted-foreground/70">
                    {chatTime(m.at)}
                  </time>
                </li>
              )
            })}
          </ol>
        )}
      </div>
      <Composer placeholder={placeholder} onSend={onSend} disabled={disabled} />
    </div>
  )
}

function Composer({ placeholder, onSend, disabled }: { placeholder: string; onSend: (text: string) => Promise<unknown>; disabled?: boolean }) {
  const [text, setText] = useState("")
  const [sending, setSending] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const box = useRef<HTMLTextAreaElement>(null)

  // Grow with the text, up to a few lines.
  useEffect(() => {
    const el = box.current
    if (!el) return
    el.style.height = "auto"
    el.style.height = `${Math.min(el.scrollHeight, 160)}px`
  }, [text])

  const send = async () => {
    const value = text.trim()
    if (!value || sending) return
    setSending(true)
    setError(null)
    try {
      await onSend(value)
      setText("")
    } catch (e) {
      setError(e instanceof Error ? e.message : "Couldn't send that.")
    } finally {
      setSending(false)
      box.current?.focus()
    }
  }

  return (
    <div className="border-t px-3 pt-3 pb-[max(0.75rem,env(safe-area-inset-bottom))] sm:px-5">
      {error && <p className="mb-2 text-sm text-destructive">{error}</p>}
      <div className="flex items-end gap-2 rounded-2xl border bg-card/70 p-1.5 pl-4 focus-within:border-primary/50">
        <textarea
          ref={box}
          value={text}
          rows={1}
          disabled={disabled}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault()
              void send()
            }
          }}
          placeholder={placeholder}
          className="max-h-40 min-h-9 flex-1 resize-none bg-transparent py-2 text-[15px] outline-none placeholder:text-muted-foreground/60 disabled:opacity-50"
        />
        <button
          type="button"
          onClick={() => void send()}
          disabled={disabled || sending || !text.trim()}
          aria-label="Send"
          className="flex size-10 shrink-0 items-center justify-center rounded-xl bg-primary text-primary-foreground transition-opacity disabled:opacity-30"
        >
          <SendHorizontal className="size-[18px]" />
        </button>
      </div>
    </div>
  )
}
