// Draw ```mermaid blocks as diagrams, in the book's light or dark theme.
(() => {
  const blocks = document.querySelectorAll("code.language-mermaid")
  if (!blocks.length) return
  const script = document.createElement("script")
  script.src = "https://cdn.jsdelivr.net/npm/mermaid@11.4.1/dist/mermaid.min.js"
  script.onload = () => {
    const dark = ["navy", "coal", "ayu"].some((t) => document.documentElement.classList.contains(t))
    window.mermaid.initialize({ startOnLoad: false, theme: dark ? "dark" : "default" })
    blocks.forEach((code) => {
      const pre = code.parentElement
      const div = document.createElement("div")
      div.className = "mermaid"
      div.textContent = code.textContent
      pre.replaceWith(div)
    })
    window.mermaid.run()
  }
  document.head.appendChild(script)
})()
