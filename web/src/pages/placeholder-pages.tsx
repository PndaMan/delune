export function PageFrame({ title, children, wide = false }: { title: string; children: React.ReactNode; wide?: boolean }) {
  return (
    <div className={wide ? "mx-auto w-full max-w-[1100px] px-5 sm:px-10" : "mx-auto w-full max-w-[900px] px-5 sm:px-10"}>
      <h1 className="type-display pt-10 pb-2 text-[44px] sm:pt-14">{title}</h1>
      {children}
    </div>
  )
}
