import { LoaderCircle } from "lucide-react"
import { useState } from "react"

import { Moon } from "@/components/moon"
import { Button } from "@/components/ui/button"
import { moonPhase } from "@/lib/moon-phase"
import { useSignIn } from "@/lib/session"
import { cn } from "@/lib/utils"

/** Signing in with a Navidrome account. delune checks the password with Navidrome and never keeps it. */
export function SignInPage() {
  const tonight = moonPhase()
  const signIn = useSignIn()
  const [username, setUsername] = useState("")
  const [password, setPassword] = useState("")

  return (
    <main className="night-sky flex min-h-dvh items-center justify-center px-5 pt-[env(safe-area-inset-top)] pb-[max(2rem,env(safe-area-inset-bottom))]">
      <div className="w-full max-w-[400px]">
        <div className="flex flex-col items-center text-center">
          <Moon illumination={Math.max(tonight.illumination, 0.12)} waxing={tonight.waxing} size={112} glow />
          <h1 className="type-display mt-8 text-[clamp(2.4rem,9vw,3.2rem)]">delune</h1>
          <p className="mt-2 text-[15px] text-muted-foreground">Sign in with your Navidrome account.</p>
        </div>

        <form
          className="mt-10 space-y-4"
          onSubmit={(e) => {
            e.preventDefault()
            if (username.trim() && password) signIn.mutate({ username: username.trim(), password })
          }}
        >
          <Field label="Username">
            <input
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              autoComplete="username"
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
              autoFocus
              required
              className={fieldClass}
            />
          </Field>
          <Field label="Password">
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              autoComplete="current-password"
              required
              className={fieldClass}
            />
          </Field>

          {signIn.isError && (
            <p role="alert" className="rounded-xl border border-destructive/30 bg-destructive/10 px-4 py-3 text-sm">
              {signIn.error.message}
            </p>
          )}

          <Button type="submit" size="lg" disabled={signIn.isPending} className="h-12 w-full rounded-xl text-[15px] font-semibold">
            {signIn.isPending && <LoaderCircle className="animate-spin" />}
            Sign in
          </Button>
        </form>

        <p className="mt-8 text-center text-[13px] leading-relaxed text-muted-foreground/80">
          Your password goes to your Navidrome server to be checked. delune doesn't store it.
        </p>
      </div>
    </main>
  )
}

const fieldClass = cn(
  "h-12 w-full rounded-xl border bg-card/70 px-4 text-[16px] outline-none transition-[border-color,box-shadow]",
  "focus:border-primary/50 focus:shadow-[0_0_0_4px_color-mix(in_oklab,var(--primary)_16%,transparent)]",
)

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="block">
      <span className="mb-1.5 block text-[13.5px] text-muted-foreground">{label}</span>
      {children}
    </label>
  )
}
