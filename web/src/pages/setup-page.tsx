import { ConnectionsForm } from "@/components/connections-form"
import { Moon } from "@/components/moon"
import { Button } from "@/components/ui/button"
import { moonPhase } from "@/lib/moon-phase"
import { type SetupStatus, skipSetup } from "@/lib/setup"

/** First run: nothing is connected yet. */
export function SetupPage({ status, onSkip }: { status: SetupStatus; onSkip: () => void }) {
  const tonight = moonPhase()
  return (
    <main className="night-sky min-h-dvh px-5 pt-[max(3rem,env(safe-area-inset-top))] pb-[max(3rem,env(safe-area-inset-bottom))]">
      <div className="mx-auto w-full max-w-[720px]">
        <div className="flex flex-col items-center text-center">
          <Moon illumination={Math.max(tonight.illumination, 0.12)} waxing={tonight.waxing} size={88} glow />
          <h1 className="type-display mt-7 text-[clamp(2.2rem,8vw,3rem)]">Welcome to delune</h1>
          <p className="mt-3 max-w-[52ch] text-[15px] text-muted-foreground">
            Connect your music folder, Navidrome and a Soulseek account. You can change any of these later in Settings.
            Until Navidrome is connected, anyone who can reach delune can use it.
          </p>
        </div>
        <div className="mt-10">
          <ConnectionsForm status={status} />
        </div>
        <div className="mt-6 text-center">
          <Button
            variant="ghost"
            onClick={() => {
              skipSetup()
              onSkip()
            }}
          >
            Skip for now
          </Button>
        </div>
      </div>
    </main>
  )
}
