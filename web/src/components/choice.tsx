import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { cn } from "@/lib/utils"

/**
 * A themed dropdown for picking one of a few values. Use it instead of `<select>`,
 * whose open list is drawn by the browser and ignores the app's colours.
 */
export function Choice<T extends string>({
  value,
  onChange,
  options,
  label,
  className,
  size = "default",
}: {
  value: T
  onChange: (value: T) => void
  options: readonly { value: T; label: string }[]
  /** Read out by screen readers. */
  label: string
  className?: string
  size?: "sm" | "default"
}) {
  return (
    <Select
      value={value}
      onValueChange={(next) => {
        if (next !== null) onChange(next as T)
      }}
      items={options}
    >
      <SelectTrigger aria-label={label} size={size} className={cn("bg-background/50", className)}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent alignItemWithTrigger={false} align="end">
        {options.map((option) => (
          <SelectItem key={option.value} value={option.value} className="py-1.5 text-[14px]">
            {option.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}
