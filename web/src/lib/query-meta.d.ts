import "@tanstack/react-query"

declare module "@tanstack/react-query" {
  interface Register {
    /** `quiet`: the screen shows this mutation's error itself, so skip the toast. */
    mutationMeta: { quiet?: boolean }
  }
}
