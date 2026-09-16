import { cn } from "@/lib/utils"

export function PageFrame({ title, children, wide = false }: { title: string; children: React.ReactNode; wide?: boolean }) {
  return (
    <div
      className={cn(
        "mx-auto w-full px-5 pb-12 sm:px-10",
        wide ? "max-w-[1100px]" : "max-w-[900px]",
      )}
    >
      <h1 className="type-display pt-10 pb-2 text-[44px] sm:pt-14">{title}</h1>
      {children}
    </div>
  )
}
