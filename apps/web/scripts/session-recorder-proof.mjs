// Independent #87 session-recorder browser acceptance (Atlas 4.1.3-4.1.5).
//
// Contract: docs/acceptance/session-recorder.md. Called by capture-park-view-proof.mjs after the
// existing park-view journey, against the same production preview and real headless Chromium.
// Failures are injected only through the declared test hooks (`?zoo-fault-injection=1`
// installing `window.__zooFaults`) or at the real browser boundary (WebGL context loss, an
// uncaught application error, throwing storage/clipboard APIs, denied downloads). Results are
// read from the real recovery panel and from `window.__zooDiagnostics.exportText()`, the same
// document that the Download/Copy controls export.
import {mkdirSync, readdirSync, readFileSync, rmSync} from "node:fs"
import {join, resolve} from "node:path"

const MAX_EXPORT_BYTES = 1_048_576
const MAX_SCREENSHOT_CHARS = 262_144
const EXPORT_KEYS = ["browser_failures", "screenshot", "session"]
const BROWSER_FAILURE_KEYS = ["message", "recovery", "source", "tick"]
const PANEL = `[aria-label="Park recovery"]`

export async function runSessionRecorderAcceptance(context) {
  const {cdp, evaluate, assert, sleep, previewUrl} = context
  const origin = new URL(previewUrl).origin
  const requests = []
  const consoleErrors = []
  const downloads = new Map()
  cdp.onEvent((event) => {
    if (event.method === "Network.requestWillBeSent") {
      requests.push({url: event.params.request.url, method: event.params.request.method, at: Date.now()})
    } else if (event.method === "Runtime.exceptionThrown") {
      consoleErrors.push({at: Date.now(), text: event.params.exceptionDetails?.exception?.description ?? event.params.exceptionDetails?.text})
    } else if (event.method === "Runtime.consoleAPICalled" && event.params.type === "error") {
      consoleErrors.push({at: Date.now(), text: event.params.args?.map((arg) => arg.value ?? arg.description).join(" ")})
    } else if (event.method === "Browser.downloadProgress") {
      downloads.set(event.params.guid, event.params.state)
    }
  })
  await cdp.send("Network.enable")
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", {enabled: false})

  const waitFor = async (description, expression, attempts = 80) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await evaluate(expression)) return
      await sleep(100)
    }
    const state = await evaluate(`({
      url: location.href,
      renderer: document.querySelector('.park-view')?.dataset.renderer ?? null,
      diagnosticsHook: typeof window.__zooDiagnostics,
      faultHook: typeof window.__zooFaults,
      panel: document.querySelector(${JSON.stringify(PANEL)})?.textContent ?? null,
      message: document.querySelector('.message')?.textContent ?? null,
    })`)
    throw new Error(`#87 acceptance timed out waiting for ${description}: ${JSON.stringify(state)}`)
  }
  const exportText = () => evaluate(`window.__zooDiagnostics.exportText()`)
  const exported = async () => JSON.parse(await exportText())
  const hudClock = () =>
    evaluate(`[...document.querySelectorAll('.stat')].find((stat) => stat.querySelector('span')?.textContent === 'Day')?.querySelector('strong')?.textContent ?? ''`)
  const panelCount = () => evaluate(`document.querySelectorAll(${JSON.stringify(PANEL)}).length`)
  const panelButton = (label) =>
    evaluate(`(() => {
      const panel = document.querySelector(${JSON.stringify(PANEL)})
      return [...(panel?.querySelectorAll('button') ?? [])].some((button) =>
        (button.getAttribute('aria-label') ?? button.textContent ?? '').includes(${JSON.stringify(label)}) && !button.disabled)
    })()`)
  const clickPanelButton = (label) =>
    evaluate(`(() => {
      const panel = document.querySelector(${JSON.stringify(PANEL)})
      const button = [...(panel?.querySelectorAll('button') ?? [])].find((candidate) =>
        (candidate.getAttribute('aria-label') ?? candidate.textContent ?? '').includes(${JSON.stringify(label)}))
      if (!button || button.disabled) return false
      button.click()
      return true
    })()`)

  const navigate = async (query = "", preload = null) => {
    let preloadId = null
    if (preload) {
      preloadId = (await cdp.send("Page.addScriptToEvaluateOnNewDocument", {source: preload})).identifier
    }
    const url = new URL(previewUrl)
    url.search = query
    await cdp.send("Page.navigate", {url: url.href})
    await waitFor("the shared 3D park renderer", `Boolean(
      window.__zooParkView?.ready && window.__zooParkView.nodeCount() > 500 &&
      document.querySelector('.park-view[data-renderer="ready"] canvas.park-canvas'))`)
    if (preloadId) await cdp.send("Page.removeScriptToEvaluateOnNewDocument", {identifier: preloadId})
    await waitFor("window.__zooDiagnostics", `typeof window.__zooDiagnostics?.exportText === 'function'`, 20)
  }

  // Accepted path drag plus a rejected fence release, through real canvas picking.
  const playOpening = async () => {
    const {clickButton, drag, tile, mouse, frame} = context
    assert(await clickButton("Ⅱ"), "#87: pause control missing")
    assert(await clickButton("Path"), "#87: path tool missing")
    await drag(await tile(3, 7), await tile(8, 7))
    assert(await clickButton("Draw habitat"), "#87: habitat tool missing")
    const from = await tile(3, 5)
    const to = await tile(5, 8)
    await mouse("mouseMoved", from)
    await mouse("mousePressed", from, {button: "left", buttons: 1, clickCount: 1})
    await mouse("mouseMoved", to, {button: "left", buttons: 1})
    await frame()
    await mouse("mouseReleased", to, {button: "left", buttons: 0, clickCount: 1})
    await frame()
    assert(await clickButton("Inspect"), "#87: inspect tool missing")
  }

  const assertCommandLog = (document, label) => {
    const log = document.session?.commands ?? []
    assert(log.some((entry) => entry.kind === "build_path_stroke" && entry.result?.ok === true),
      `${label}: accepted path command missing from exported log`, log.slice(-5))
    assert(log.some((entry) => entry.kind === "place_habitat_rect" && entry.result?.ok === false),
      `${label}: rejected fence command missing from exported log`, log.slice(-5))
  }

  const assertExportShape = (text, document, label) => {
    assert(text.length <= MAX_EXPORT_BYTES, `${label}: export exceeds 1 MB`, text.length)
    assert(JSON.stringify(Object.keys(document).sort()) === JSON.stringify(EXPORT_KEYS),
      `${label}: export has undeclared top-level fields`, Object.keys(document))
    assert(Array.isArray(document.browser_failures), `${label}: browser_failures missing`)
    for (const failure of document.browser_failures) {
      assert(JSON.stringify(Object.keys(failure).sort()) === JSON.stringify(BROWSER_FAILURE_KEYS),
        `${label}: browser failure has undeclared fields`, failure)
    }
    assert(document.screenshot === null ||
      (typeof document.screenshot === "string" && document.screenshot.startsWith("data:image/png;base64,") &&
        document.screenshot.length <= MAX_SCREENSHOT_CHARS),
      `${label}: screenshot must be null or a size-capped PNG data URL`, String(document.screenshot).slice(0, 64))
    assert(document.session?.format === "zoo-diagnostics/1", `${label}: Rust session bundle missing`, document.session?.format)
  }

  // A frozen or paused clock must not move while the player keeps asking for speed.
  const assertClockStill = async (label, pressSpeed = true) => {
    const before = await hudClock()
    if (pressSpeed) await context.clickButton("4×")
    await sleep(1500)
    const after = await hudClock()
    assert(before && before === after, `${label}: simulation clock advanced`, {before, after})
  }

  const assertNoLoop = async (label, countFailures) => {
    const failuresBefore = await countFailures()
    const errorsBefore = consoleErrors.length
    await sleep(2000)
    assert(await panelCount() === 1, `${label}: recovery panel duplicated or vanished`, await panelCount())
    assert(await countFailures() === failuresBefore, `${label}: failure records kept growing`,
      {failuresBefore, after: await countFailures()})
    assert(consoleErrors.length - errorsBefore <= 2, `${label}: failure handling is looping`,
      consoleErrors.slice(errorsBefore, errorsBefore + 5))
  }

  const assertCompactPanel = async (label) => {
    const area = await evaluate(`(() => {
      const rect = document.querySelector(${JSON.stringify(PANEL)}).getBoundingClientRect()
      return (rect.width * rect.height) / (innerWidth * innerHeight)
    })()`)
    assert(area > 0 && area < 0.4, `${label}: recovery panel is not compact`, area)
    assert(await panelButton("Download diagnostics"), `${label}: Download diagnostics control missing`)
    assert(await panelButton("Copy diagnostics"), `${label}: Copy diagnostics control missing`)
  }

  const noUploads = (since, label) => {
    const offending = requests.filter((request) => request.at >= since &&
      (request.method !== "GET" || (!request.url.startsWith(origin) && !request.url.startsWith("data:") && !request.url.startsWith("blob:"))))
    assert(offending.length === 0, `${label}: diagnostics were sent over the network`, offending)
  }

  // --- 1. Normal play: hooks, explicit export, camera stays out of the log ------------------------
  await navigate()
  assert(await evaluate(`typeof window.__zooFaults === 'undefined'`),
    "normal play must not install the fault-injection hook")
  await playOpening()
  const beforeCamera = (await exported()).session
  const {wheel, drag, key, tile} = context
  const center = await tile(10, 7)
  await wheel(center, -400)
  await drag(center, {x: center.x + 160, y: center.y - 40}, "right")
  await key("e", 3)
  await key("Home")
  const afterCamera = (await exported()).session
  assert(afterCamera.commands_total === beforeCamera.commands_total,
    "camera orbit/zoom/reset added authoritative commands",
    {before: beforeCamera.commands_total, after: afterCamera.commands_total, tail: afterCamera.commands.slice(-3)})
  assertCommandLog(await exported(), "normal play")
  assert(await context.clickButton("Export diagnostics"), "normal play: Export diagnostics control missing")
  await waitFor("the recovery/export panel in normal play", `Boolean(document.querySelector(${JSON.stringify(PANEL)}))`, 20)
  await assertCompactPanel("normal play")
  assert(!(await panelButton("Continue paused")), "normal play: no failure, so no continue-paused choice")
  assert(await clickPanelButton("Close"), "normal play: export panel cannot be closed")
  await waitFor("the export panel to close", `!document.querySelector(${JSON.stringify(PANEL)})`, 20)
  assert((await exported()).session.status === "running", "opening the export panel froze the session")

  // --- 2. WASM panic: freeze, retained bundle, download + copy, privacy, no loop ------------------
  const cookieSecret = `zoo87-cookie-${process.pid}`
  const storageSecret = `zoo87-storage-${process.pid}`
  const querySecret = `zoo87-query-${process.pid}`
  await cdp.send("Network.setCookie", {name: "zoo87_secret", value: cookieSecret, url: previewUrl})
  await cdp.send("Browser.grantPermissions", {origin, permissions: ["clipboardReadWrite", "clipboardSanitizedWrite"]})
  await navigate(`?zoo-fault-injection=1&zoo87_secret=${querySecret}`)
  await evaluate(`localStorage.setItem('zoo87_secret', ${JSON.stringify(storageSecret)})`)
  assert(await evaluate(`typeof window.__zooFaults?.simulation === 'function'`),
    "?zoo-fault-injection=1 did not install window.__zooFaults")
  await playOpening()
  assert(await context.clickButton("4×"), "4x speed control missing")
  const panicAt = Date.now()
  await evaluate(`window.__zooFaults.simulation("panic")`)
  await waitFor("the recovery panel after a WASM panic", `Boolean(document.querySelector(${JSON.stringify(PANEL)}))`)
  await assertCompactPanel("wasm panic")
  assert(!(await panelButton("Continue paused")), "wasm panic: a broken simulation must not offer continue-paused")
  await assertClockStill("wasm panic")
  const panicText = await exportText()
  const panicDocument = JSON.parse(panicText)
  assertExportShape(panicText, panicDocument, "wasm panic")
  assert(panicDocument.browser_failures.some((failure) => failure.source === "wasm" && failure.recovery === "frozen"),
    "wasm panic: failure record missing from exported bundle", panicDocument.browser_failures)
  assertCommandLog(panicDocument, "wasm panic")
  for (const secret of [cookieSecret, storageSecret, querySecret]) {
    assert(!panicText.includes(secret), "exported diagnostics contain unrelated browser data", secret)
  }
  await assertNoLoop("wasm panic", async () => (await exported()).browser_failures.length)
  const downloadDir = resolve("test-results/session-downloads")
  rmSync(downloadDir, {recursive: true, force: true})
  mkdirSync(downloadDir, {recursive: true})
  await cdp.send("Browser.setDownloadBehavior", {behavior: "allowAndName", downloadPath: downloadDir, eventsEnabled: true})
  assert(await clickPanelButton("Download diagnostics"), "wasm panic: Download diagnostics not clickable")
  for (let attempt = 0; attempt < 50 && ![...downloads.values()].includes("completed"); attempt += 1) await sleep(100)
  const files = readdirSync(downloadDir)
  assert(files.length === 1, "wasm panic: Download diagnostics produced no single file", {files, downloads: [...downloads]})
  assert(JSON.stringify(JSON.parse(readFileSync(join(downloadDir, files[0]), "utf8"))) === JSON.stringify(JSON.parse(await exportText())),
    "wasm panic: downloaded file differs from window.__zooDiagnostics.exportText()")
  await cdp.send("Page.bringToFront")
  assert(await clickPanelButton("Copy diagnostics"), "wasm panic: Copy diagnostics not clickable")
  await sleep(300)
  const copied = await evaluate(`navigator.clipboard.readText()`)
  assert(JSON.stringify(JSON.parse(copied)) === JSON.stringify(JSON.parse(await exportText())),
    "wasm panic: copied text differs from window.__zooDiagnostics.exportText()")
  noUploads(panicAt, "wasm panic")
  await context.screenshot("test-results/session-recovery-wasm-panic.png")
  await cdp.send("Browser.setDownloadBehavior", {behavior: "default"})
  await evaluate(`localStorage.removeItem('zoo87_secret')`)
  await cdp.send("Network.deleteCookies", {name: "zoo87_secret", url: previewUrl})

  // --- 3. Detected invariant violation: frozen Rust session with its failure record --------------
  await navigate("?zoo-fault-injection=1")
  await playOpening()
  assert(await context.clickButton("4×"), "4x speed control missing")
  await evaluate(`window.__zooFaults.simulation("invariant")`)
  await waitFor("the recovery panel after an invariant violation", `Boolean(document.querySelector(${JSON.stringify(PANEL)}))`)
  await assertCompactPanel("invariant violation")
  assert(!(await panelButton("Continue paused")), "invariant violation: frozen session must not offer continue-paused")
  await assertClockStill("invariant violation")
  const invariantText = await exportText()
  const invariantDocument = JSON.parse(invariantText)
  assertExportShape(invariantText, invariantDocument, "invariant violation")
  assert(invariantDocument.session.status === "frozen", "invariant violation: Rust session not frozen", invariantDocument.session.status)
  const [invariant, ...extra] = invariantDocument.session.failures
  assert(invariant?.kind === "invariant_violation" && invariant.source === "simulation" && invariant.injected === true && extra.length === 0,
    "invariant violation: exported failure record missing or duplicated", invariantDocument.session.failures)
  assertCommandLog(invariantDocument, "invariant violation")
  await assertNoLoop("invariant violation", async () => (await exported()).session.failures.length)
  await context.screenshot("test-results/session-recovery-invariant.png")

  // --- 4. Renderer failure (real WebGL context loss): continue paused, simulation intact ----------
  await navigate()
  await playOpening()
  assert(await context.clickButton("1×"), "1x speed control missing")
  await evaluate(`(() => {
    const canvas = document.querySelector('canvas.park-canvas')
    const gl = canvas.getContext('webgl2') ?? canvas.getContext('webgl')
    gl.getExtension('WEBGL_lose_context').loseContext()
  })()`)
  await waitFor("the recovery panel after a renderer failure", `Boolean(document.querySelector(${JSON.stringify(PANEL)}))`)
  await assertCompactPanel("renderer failure")
  assert(await panelButton("Continue paused"), "renderer failure: continue-paused option missing")
  await assertClockStill("renderer failure (panel open)", false)
  const rendererText = await exportText()
  const rendererDocument = JSON.parse(rendererText)
  assertExportShape(rendererText, rendererDocument, "renderer failure")
  assert(rendererDocument.browser_failures.some((failure) => failure.source === "renderer" && failure.recovery === "continue_paused"),
    "renderer failure: failure record missing from exported bundle", rendererDocument.browser_failures)
  assert(rendererDocument.session.status === "running" && rendererDocument.session.failures.length === 0,
    "renderer failure must leave the simulation intact", rendererDocument.session)
  assertCommandLog(rendererDocument, "renderer failure")
  await assertNoLoop("renderer failure", async () => (await exported()).browser_failures.length)
  await context.screenshot("test-results/session-recovery-renderer.png")
  assert(await clickPanelButton("Continue paused"), "renderer failure: continue-paused not clickable")
  await waitFor("the panel to close after continue-paused", `!document.querySelector(${JSON.stringify(PANEL)})`, 20)
  await assertClockStill("renderer failure (continued paused)", false)
  await sleep(1500)
  assert(await panelCount() === 0, "renderer failure: panel reopened by itself (failure loop)")
  assert((await exported()).browser_failures.filter((failure) => failure.source === "renderer").length === 1,
    "renderer failure: renderer failure recorded more than once")
  const pausedClock = await hudClock()
  assert(await context.clickButton("1×"), "renderer failure: speed control unavailable after continue-paused")
  await sleep(1500)
  assert((await hudClock()) !== pausedClock, "renderer failure: simulation could not resume after continue-paused")

  // --- 5. Uncaught application failure stops advancing and is recorded ---------------------------
  await navigate()
  await playOpening()
  assert(await context.clickButton("4×"), "4x speed control missing")
  await evaluate(`setTimeout(() => { throw new Error("zoo-87-acceptance-uncaught") }, 0)`)
  await waitFor("the recovery panel after an uncaught application error", `Boolean(document.querySelector(${JSON.stringify(PANEL)}))`)
  await assertCompactPanel("application failure")
  const applicationBefore = await hudClock()
  await sleep(1500)
  assert((await hudClock()) === applicationBefore, "application failure: simulation kept advancing")
  const applicationDocument = await exported()
  assert(applicationDocument.browser_failures.some((failure) => failure.source === "application" &&
    failure.message.includes("zoo-87-acceptance-uncaught")),
  "application failure: failure record missing from exported bundle", applicationDocument.browser_failures)
  await assertNoLoop("application failure", async () => (await exported()).browser_failures.length)

  // --- 6. Storage failure: throwing storage/clipboard, denied download -> manual export ----------
  const storageFailure = `(() => {
    const deny = () => { throw new DOMException("zoo-87 acceptance: storage denied", "SecurityError") }
    for (const name of ["localStorage", "sessionStorage"]) Object.defineProperty(window, name, {configurable: true, get: deny})
    Object.defineProperty(window, "indexedDB", {configurable: true, get: deny})
    if (navigator.clipboard) {
      Object.defineProperty(navigator.clipboard, "writeText", {configurable: true,
        value: () => Promise.reject(new DOMException("zoo-87 acceptance: clipboard denied", "NotAllowedError"))})
    }
  })()`
  await cdp.send("Browser.setDownloadBehavior", {behavior: "deny"})
  await navigate("", storageFailure)
  await playOpening()
  assert(await context.clickButton("Export diagnostics"), "storage failure: Export diagnostics control missing")
  await waitFor("the export panel with storage unavailable", `Boolean(document.querySelector(${JSON.stringify(PANEL)}))`, 20)
  assert(await clickPanelButton("Download diagnostics"), "storage failure: Download diagnostics not clickable")
  assert(await clickPanelButton("Copy diagnostics"), "storage failure: Copy diagnostics not clickable")
  await waitFor("the manual export fallback", `Boolean(document.querySelector('textarea[aria-label="Diagnostics bundle"]')?.value)`, 30)
  const manual = JSON.parse(await evaluate(`document.querySelector('textarea[aria-label="Diagnostics bundle"]').value`))
  assert(manual.session?.format === "zoo-diagnostics/1", "storage failure: manual fallback does not hold the bundle")
  assertCommandLog(manual, "storage failure")
  const storageDocument = await exported()
  const storageFailures = () => exported().then((document) => document.browser_failures.filter((failure) => failure.source === "storage").length)
  assert(storageDocument.browser_failures.some((failure) => failure.source === "storage" && failure.recovery === "manual_export"),
    "storage failure: failure record missing from exported bundle", storageDocument.browser_failures)
  for (let index = 0; index < 3; index += 1) await clickPanelButton("Copy diagnostics")
  await sleep(500)
  assert(await storageFailures() <= 5, "storage failure: retries produce unbounded failure records", await storageFailures())
  await assertNoLoop("storage failure", storageFailures)
  assert(storageDocument.session.status === "running", "storage failure must not freeze the simulation")
  await context.screenshot("test-results/session-recovery-storage.png")
  await cdp.send("Browser.setDownloadBehavior", {behavior: "default"})

  console.log(
    "Session recorder acceptance passed: hooks gated, camera outside the log, explicit export, WASM panic and invariant freeze, renderer continue-paused, application and storage failures recorded, download/copy/manual export, no loops, no uploads.",
  )
}
