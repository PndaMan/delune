import { Moon } from "@/components/moon"

type Props = {
  illumination: number
  title: string
  children: React.ReactNode
  action?: React.ReactNode
}

/** A quiet, centred message with a moon: used wherever there's nothing to show yet. */
export function EmptyState({ illumination, title, children, action }: Props) {
  return (
    <div className="mx-auto flex max-w-md flex-col items-center px-6 py-20 text-center">
      <Moon illumination={illumination} size={72} glow />
      <h2 className="type-title mt-7 text-[26px] text-balance">{title}</h2>
      <div className="mt-3 text-[15px] leading-relaxed text-pretty text-muted-foreground">{children}</div>
      {action && <div className="mt-7">{action}</div>}
    </div>
  )
}
