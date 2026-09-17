import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { ChevronRight, FileAudio, Folder, FolderOpen, LoaderCircle, ScanSearch } from "lucide-react"
import { useRef, useState } from "react"

import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { api, type NamingOptions } from "@/lib/api"
import { plural } from "@/lib/format"
import { type DetectedLayout, type NamingSettings, namingApi, useNamingSettings } from "@/lib/naming"
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

/** Edit the template imports follow; people who manage delune can save it. */
export function NamingEditor({ editable }: { editable: boolean }) {
  const client = useQueryClient()
  const saved = useNamingSettings()
  const [draft, setDraft] = useState<NamingSettings | null>(null)
  const [detected, setDetected] = useState<DetectedLayout | null>(null)
  const current = draft ?? saved.data ?? { template: PRESETS[0].template, options: DEFAULTS }
  const { template, options } = current
  const setTemplate = (next: string) => setDraft({ ...current, template: next })
  const setOptions = (next: NamingOptions) => setDraft({ ...current, options: next })
  const dirty =
    draft !== null &&
    (draft.template !== saved.data?.template || JSON.stringify(draft.options) !== JSON.stringify(saved.data?.options))

  const save = useMutation({
    meta: { quiet: true },
    mutationFn: namingApi.update,
    onSuccess: (next) => {
      client.setQueryData(["naming"], next)
      setDraft(null)
      setDetected(null)
      void client.invalidateQueries({ queryKey: ["downloads"] })
    },
  })
  const detect = useMutation({
    meta: { quiet: true },
    mutationFn: namingApi.detect,
    onSuccess: (layout) => {
      setDetected(layout)
      setDraft({ template: layout.template, options: { ...options, ...layout.options } })
    },
  })
  const input = useRef<HTMLInputElement>(null)

  const tokens = useQuery({
    queryKey: ["naming-tokens"],
    queryFn: ({ signal }) => api.namingTokens(signal),
    staleTime: Infinity,
  })
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
      <div className="min-w-0 space-y-7">
        {editable && (
          <div className="flex flex-wrap items-center gap-3 rounded-2xl bg-primary/8 px-4 py-3 ring-1 ring-primary/20">
            <ScanSearch className="size-5 shrink-0 text-primary" />
            <p className="min-w-0 flex-1 text-[14px]">Name new albums the way your library already is.</p>
            <Button variant="outline" size="sm" disabled={detect.isPending} onClick={() => detect.mutate()}>
              {detect.isPending && <LoaderCircle className="animate-spin" />} Match my library
            </Button>
            {detect.isError && <p className="w-full text-sm text-destructive">{detect.error.message}</p>}
            {detected && (
              <p className="w-full text-[13px] text-muted-foreground">
                {detected.matching === detected.sampled
                  ? `All ${plural(detected.sampled, "file")} checked follow this layout`
                  : `${detected.matching} of ${plural(detected.sampled, "file")} checked follow this layout`}
                {dirty ? ". It's filled in; save to use it." : ", which delune already uses."}
              </p>
            )}
          </div>
        )}

        <div role="radiogroup" aria-label="Start from" className="grid gap-2 sm:grid-cols-3">
          {PRESETS.map((preset) => {
            const chosen = template === preset.template
            return (
              <button
                key={preset.label}
                type="button"
                role="radio"
                aria-checked={chosen}
                onClick={() => setTemplate(preset.template)}
                className={cn(
                  "rounded-2xl border px-3.5 py-3 text-left outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring",
                  chosen ? "border-primary bg-primary/10" : "bg-card/40 hover:border-foreground/25",
                )}
              >
                <span className="block text-[14px] font-medium">{preset.label}</span>
                <Shape template={preset.template} />
              </button>
            )
          })}
        </div>

        <div>
          <label htmlFor="naming-template" className="text-[14px] font-medium">
            Template
          </label>
          <Input
            id="naming-template"
            ref={input}
            value={template}
            onChange={(e) => setTemplate(e.target.value)}
            spellCheck={false}
            aria-invalid={error ? true : undefined}
            className="mt-2 h-12 rounded-xl bg-card/60 text-[15px]"
          />
          {error ? (
            <p className="mt-2 text-sm text-destructive">
              {capitalise(error.message)}:{" "}
              <span className="text-muted-foreground">
                {[...template].slice(0, error.position).join("")}
                <mark className="rounded bg-destructive/20 px-0.5 text-destructive">
                  {[...template].slice(error.position, error.position + 1).join("") || " "}
                </mark>
                {[...template].slice(error.position + 1).join("")}
              </span>
            </p>
          ) : (
            <Levels template={template} />
          )}
          <p className="mt-3 text-[13px] text-muted-foreground">
            Tap a field to add it where the cursor is. Text in [square brackets] is left out when a field inside is
            empty.
          </p>
          <div className="mt-2 flex flex-wrap gap-1.5">
            {tokens.data?.map((t) => (
              <Tooltip key={t.name}>
                <TooltipTrigger
                  render={
                    <button
                      type="button"
                      onClick={() => insert(t.name)}
                      className="rounded-lg border border-primary/25 bg-primary/8 px-2.5 py-1 text-[13px] text-foreground/90 transition-colors outline-none hover:border-primary/60 hover:bg-primary/15 focus-visible:ring-2 focus-visible:ring-ring"
                    />
                  }
                >
                  {t.name.replace(/_/g, " ")}
                </TooltipTrigger>
                <TooltipContent>{t.description}</TooltipContent>
              </Tooltip>
            ))}
          </div>
        </div>

        <section aria-label="How imports will be named">
          <Tree examples={examples ?? []} loading={!examples} dimmed={!!error} />
        </section>

        <div className="grid gap-x-6 gap-y-5 sm:grid-cols-2">
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
            label="Albums on several discs"
            value={options.multi_disc}
            onChange={(v) => setOptions({ ...options, multi_disc: v as NamingOptions["multi_disc"] })}
            choices={[
              ["disc-prefix", "2-03"],
              ["continuous", "15"],
              ["per-disc", "03"],
            ]}
          />
          <Choice
            label="Spaces in names"
            value={options.whitespace}
            onChange={(v) => setOptions({ ...options, whitespace: v as NamingOptions["whitespace"] })}
            choices={[
              ["preserve", "Keep"],
              ["collapse", "Tidy"],
              ["underscore", "Use _"],
            ]}
          />
          <div>
            <label htmlFor="naming-replacement" className="block text-sm text-muted-foreground">
              Characters like / : ? become
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

        {!editable && (
          <p className="text-sm text-muted-foreground">Someone who manages delune can change how files are named.</p>
        )}
      </div>

      {editable && (dirty || save.isError) && (
        <div className="sticky bottom-[calc(var(--chrome-bottom)+1rem)] z-10 flex flex-wrap items-center gap-3 rounded-2xl border bg-card/95 px-5 py-3 shadow-lg backdrop-blur">
          <p className="min-w-0 flex-1 text-sm text-muted-foreground">
            {save.isError ? (
              <span className="text-destructive">{save.error.message}</span>
            ) : (
              "Albums waiting in Review are re-planned when you save."
            )}
          </p>
          <Button
            variant="ghost"
            onClick={() => {
              setDraft(null)
              setDetected(null)
            }}
            disabled={save.isPending}
          >
            Discard
          </Button>
          <Button onClick={() => save.mutate(current)} disabled={!!error || save.isPending}>
            {save.isPending && <LoaderCircle className="animate-spin" />} Save naming
          </Button>
        </div>
      )}
    </div>
  )
}

const LEVEL_NAMES = ["Artist folder", "Album folder", "Disc folder"]

/** The template split into what it makes: a folder per level, then the file. */
function Levels({ template }: { template: string }) {
  const parts = template.split("/")
  return (
    <ol className="mt-3 flex flex-wrap items-center gap-1.5 text-[13px]">
      {parts.map((part, i) => {
        const file = i === parts.length - 1
        return (
          <li key={`${i}-${part}`} className="flex items-center gap-1.5">
            {i > 0 && <ChevronRight className="size-3.5 text-muted-foreground/60" />}
            <span className="flex items-center gap-1.5 rounded-lg bg-card/70 px-2 py-1 ring-1 ring-border">
              {file ? (
                <FileAudio className="size-3.5 text-muted-foreground" />
              ) : (
                <Folder className="size-3.5 text-muted-foreground" />
              )}
              <span className="text-muted-foreground">{file ? "File" : (LEVEL_NAMES[i] ?? "Folder")}</span>
              <Tokens text={part} />
            </span>
          </li>
        )
      })}
    </ol>
  )
}

/** Template text with its fields picked out. */
function Tokens({ text }: { text: string }) {
  const pieces = text.split(/(\{[^{}]+\})/g).filter(Boolean)
  return (
    <span className="break-all">
      {pieces.map((piece, i) =>
        piece.startsWith("{") ? (
          <span key={i} className="rounded bg-primary/15 px-1 text-primary">
            {piece.slice(1, -1).replace(/_/g, " ")}
          </span>
        ) : (
          <span key={i}>{piece}</span>
        ),
      )}
    </span>
  )
}

/** A preset's folder shape, small: how many levels, and what the file is called. */
function Shape({ template }: { template: string }) {
  const parts = template.split("/")
  return (
    <span className="mt-2 block space-y-0.5 text-[12px] text-muted-foreground">
      {parts.map((part, i) => (
        <span key={`${i}-${part}`} className="flex items-center gap-1.5" style={{ paddingLeft: `${i * 10}px` }}>
          {i === parts.length - 1 ? (
            <FileAudio className="size-3 shrink-0" />
          ) : (
            <Folder className="size-3 shrink-0" />
          )}
          <span className="truncate">{part.replace(/[{}[\]]/g, "").replace(/_/g, " ")}</span>
        </span>
      ))}
    </span>
  )
}

type TreeNode = { name: string; children: TreeNode[]; label?: string }

function buildTree(examples: { label: string; path: string }[]): TreeNode[] {
  const root: TreeNode[] = []
  for (const example of examples) {
    let level = root
    const parts = example.path.split("/")
    parts.forEach((part, i) => {
      let node = level.find((n) => n.name === part)
      if (!node) {
        node = { name: part, children: [] }
        level.push(node)
      }
      if (i === parts.length - 1) node.label = example.label
      level = node.children
    })
  }
  return root
}

/** What imports will look like in the library, as folders and files. */
function Tree({
  examples,
  loading,
  dimmed,
}: {
  examples: { label: string; path: string }[]
  loading: boolean
  dimmed: boolean
}) {
  const tree = buildTree(examples)
  return (
    <div className="overflow-hidden rounded-2xl border bg-[color-mix(in_oklab,var(--card)_70%,black)]">
      <div className="flex items-center gap-2 border-b px-4 py-2.5">
        <FolderOpen className="size-4 text-primary" />
        <span className="text-[13.5px] font-medium">Your music folder</span>
        <span className="ml-auto text-[12px] text-muted-foreground">Changes as you edit</span>
      </div>
      {loading ? (
        <div className="h-48 animate-pulse bg-muted/20" />
      ) : (
        <ul className={cn("px-3 py-3 text-[13.5px] transition-opacity", dimmed && "opacity-40")}>
          {tree.map((node) => (
            <TreeRow key={node.name} node={node} depth={0} />
          ))}
        </ul>
      )}
    </div>
  )
}

function TreeRow({ node, depth }: { node: TreeNode; depth: number }) {
  const file = node.children.length === 0
  return (
    <li>
      <div className="flex min-w-0 items-start gap-2 rounded-md py-1 pr-1" style={{ paddingLeft: `${depth * 16 + 4}px` }}>
        {file ? (
          <FileAudio className="mt-0.5 size-4 shrink-0 text-primary" />
        ) : (
          <Folder className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
        )}
        <span className={cn("min-w-0 flex-1 break-words", file ? "text-foreground" : "text-muted-foreground")}>
          {node.name}
          {file && node.label && <span className="block text-[11.5px] text-muted-foreground/70">{node.label}</span>}
        </span>
      </div>
      {!file && (
        <ul>
          {node.children.map((child) => (
            <TreeRow key={child.name} node={child} depth={depth + 1} />
          ))}
        </ul>
      )}
    </li>
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
            className="h-8 min-w-14 rounded-lg px-3 text-sm whitespace-nowrap text-muted-foreground transition-colors outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring aria-checked:bg-accent aria-checked:text-foreground"
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
