/**
 * Minimal Server-Sent Events reader over fetch.
 *
 * `EventSource` can't read error bodies and reconnects on its own, which would
 * silently restart a finished search. Reading the stream ourselves avoids both.
 */
export async function readEventStream(
  response: Response,
  onData: (data: string) => void,
  signal: AbortSignal,
): Promise<void> {
  if (!response.body) return
  const reader = response.body.pipeThrough(new TextDecoderStream()).getReader()
  let buffer = ""
  let data: string[] = []
  signal.addEventListener("abort", () => void reader.cancel().catch(() => {}), { once: true })

  for (;;) {
    const { value, done } = await reader.read()
    if (done) return
    buffer += value
    let newline: number
    while ((newline = buffer.indexOf("\n")) >= 0) {
      const line = buffer.slice(0, newline).replace(/\r$/, "")
      buffer = buffer.slice(newline + 1)
      if (line === "") {
        if (data.length) onData(data.join("\n"))
        data = []
      } else if (line.startsWith("data:")) {
        data.push(line.slice(line[5] === " " ? 6 : 5))
      }
    }
  }
}
