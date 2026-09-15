import { useQuery } from "@tanstack/react-query"
import { useRef, useState } from "react"

import { Input } from "@/components/ui/input"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { api, type NamingOptions } from "@/lib/api"
import { cn } from "@/lib/utils"

const PRESETS = [
  { label: "Artist, year and album", template: "{album_artist}/{year} - {album}[ ({edition})]/{track} - {title}" },
  { label: "Album with quality", template: "{album_artist}/{album} ({year}) [[{quality}]]/{track} {title}" },
  { label: "Everything in one folder", template: "{album_artist} - {album} - {track} - {title}" },
]

const DEFAULTS: NamingOptions = {
  track_padding: 2,
  multi_disc: "disc-prefix",
  illegal_replacement: "_",
  whitespace: "preserve",
  max_component_bytes: 200,
}

export function NamingEditor() {
  const input = useRef<HTMLInputElement>(null)
  const [template, setTemplate] = useState(PRESETS[0].template)
  const [options, setOptions] = useState(DEFAULTS)

  const tokens = useQuery({ queryKey: ["naming-tokens"], queryFn: ({ signal }) => api.namingTokens(signal), staleTime: Infinity })
  const preview = useQuery({
    queryKey: ["naming-preview", template, options],
    queryFn: ({ signal }) => api.namingPreview(template, options, signal),
    placeholderData: (previous) => previous,
  })

  const error = preview.data && "error" in preview.data ? preview.data.error : null
  const examples = preview.data && "examples" in preview.data ? preview.data.examples : null

  const insert = (token: string) => {
    const el = input.current
    const text = `{${token}}`
    const start = el?.selectionStart ?? template.length
    const end = el?.selectionEnd ?? template.length
    setTemplate(template.slice(0, start) + text + template.slice(end))
    requestAnimationFrame(() => {
      el?.focus()
      el?.setSelectionRange(start + text.length, start + text.length)
    })
  }

  return (
    <div className="space-y-7">
      <div className="flex flex-wrap gap-2">
        {PRESETS.map((preset) => (
          <button
            key={preset.label}
            type="button"
            onClick={() => setTemplate(preset.template)}
            aria-pressed={template === preset.template}
            className="rounded-full border px-3.5 py-1.5 text-sm text-muted-foreground transition-colors outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring aria-pressed:border-primary/50 aria-pressed:bg-primary/10 aria-pressed:text-foreground"
          >
            {preset.label}
          </button>
        ))}
      </div>

      <div>
        <label htmlFor="naming-template" className="text-sm text-muted-foreground">
          Template
        </label>
        <Input
          id="naming-template"
          ref={input}
          value={template}
          onChange={(e) => setTemplate(e.target.value)}
          spellCheck={false}
          aria-invalid={error ? true : undefined}
          className="mt-2 h-12 rounded-xl bg-card/60 text-[15.5px]"
        />
        {error ? (
          <p className="mt-2 text-sm text-destructive">
            {capitalise(error.message)}:{" "}
            <span className="text-muted-foreground">
              {[...template].slice(0, error.position).join("")}
              <mark className="rounded bg-destructive/20 px-0.5 text-destructive">{[...template].slice(error.position, error.position + 1).join("") || " "}</mark>
              {[...template].slice(error.position + 1).join("")}
            </span>
          </p>
        ) : (
          <p className="mt-2 text-sm text-muted-foreground">
            Use / for folders. Text in [square brackets] disappears when a token inside it is empty.
          </p>
        )}
      </div>

      <div className="flex flex-wrap gap-1.5">
        {tokens.data?.map((t) => (
          <Tooltip key={t.name}>
            <TooltipTrigger
              render={
                <button
                  type="button"
                  onClick={() => insert(t.name)}
                  className="rounded-lg bg-muted/70 px-2.5 py-1 text-[13.5px] text-foreground/85 transition-colors outline-none hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
                />
              }
            >
              {t.name}
            </TooltipTrigger>
            <TooltipContent>{t.description}</TooltipContent>
          </Tooltip>
        ))}
      </div>

      <div className="grid gap-5 sm:grid-cols-2">
        <Choice
          label="Track numbers"
          value={String(options.track_padding)}
          onChange={(v) => setOptions({ ...options, track_padding: Number(v) })}
          choices={[
            ["1", "1"],
            ["2", "01"],
            ["3", "001"],
          ]}
        />
        <Choice
          label="Multi-disc albums"
          value={options.multi_disc}
          onChange={(v) => setOptions({ ...options, multi_disc: v as NamingOptions["multi_disc"] })}
          choices={[
            ["disc-prefix", "2-03"],
            ["continuous", "15"],
            ["per-disc", "03"],
          ]}
        />
        <Choice
          label="Spaces"
          value={options.whitespace}
          onChange={(v) => setOptions({ ...options, whitespace: v as NamingOptions["whitespace"] })}
          choices={[
            ["preserve", "Keep"],
            ["collapse", "Tidy"],
            ["underscore", "Use _"],
          ]}
        />
        <div>
          <label htmlFor="naming-replacement" className="text-sm text-muted-foreground">
            Replace characters like / : ? with
          </label>
          <Input
            id="naming-replacement"
            value={options.illegal_replacement}
            maxLength={3}
            onChange={(e) => setOptions({ ...options, illegal_replacement: e.target.value })}
            className="mt-2 h-10 w-24 rounded-xl bg-card/60 text-center"
          />
        </div>
      </div>

      <div className="overflow-hidden rounded-2xl border bg-card/50">
        {(examples ?? []).map((example) => (
          <div key={example.label} className="border-b px-5 py-4 last:border-b-0">
            <p className="text-[12.5px] text-muted-foreground">{example.label}</p>
            <PathPreview path={example.path} dimmed={!!error} />
          </div>
        ))}
        {!examples && <div className="h-40 animate-pulse bg-muted/30" />}
      </div>
    </div>
  )
}

function PathPreview({ path, dimmed }: { path: string; dimmed: boolean }) {
  const parts = path.split("/")
  return (
    <p className={cn("mt-1 text-[15px] break-words transition-opacity", dimmed && "opacity-40")}>
      {parts.map((part, i) => (
        <span key={`${i}-${part}`}>
          {i > 0 && <span className="px-1 text-muted-foreground/50">/</span>}
          <span className={i === parts.length - 1 ? "text-foreground" : "text-muted-foreground"}>{part}</span>
        </span>
      ))}
    </p>
  )
}

function Choice({
  label,
  value,
  onChange,
  choices,
}: {
  label: string
  value: string
  onChange: (value: string) => void
  choices: [string, string][]
}) {
  return (
    <div>
      <p className="text-sm text-muted-foreground">{label}</p>
      <div role="radiogroup" aria-label={label} className="mt-2 inline-flex rounded-xl border bg-card/60 p-1">
        {choices.map(([id, text]) => (
          <button
            key={id}
            type="button"
            role="radio"
            aria-checked={value === id}
            onClick={() => onChange(id)}
            className="h-8 min-w-14 rounded-lg px-3 text-sm text-muted-foreground transition-colors outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring aria-checked:bg-accent aria-checked:text-foreground"
          >
            {text}
          </button>
        ))}
      </div>
    </div>
  )
}

function capitalise(s: string) {
  return s.charAt(0).toUpperCase() + s.slice(1)
}
