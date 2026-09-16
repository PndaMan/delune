import { cn } from "@/lib/utils"

export function PageFrame({
  title,
  children,
  wide = false,
  action,
}: {
  title: string
  children: React.ReactNode
  wide?: boolean
  /** Shown beside the title, like an "add" button. */
  action?: React.ReactNode
}) {
  return (
    <div
      className={cn(
        "mx-auto w-full px-5 pb-12 sm:px-10",
        wide ? "max-w-[1100px]" : "max-w-[900px]",
      )}
    >
      <div className="flex items-end justify-between gap-4 pt-10 pb-2 sm:pt-14">
        <h1 className="type-display text-[44px]">{title}</h1>
        {action && <div className="shrink-0 pb-1.5">{action}</div>}
      </div>
      {children}
    </div>
  )
}
