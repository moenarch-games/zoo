import {spawn} from "node:child_process"
import {existsSync, mkdirSync, rmSync, writeFileSync} from "node:fs"

const previewUrl = process.env.PREVIEW_URL ?? "http://127.0.0.1:4173/"
const contentAcceptance = process.env.ZOO_CONTENT_ACCEPTANCE
const debuggingPort = Number(process.env.CHROME_DEBUG_PORT ?? 9231)
const chromeCandidates = [
  process.env.CHROME_PATH,
  "/usr/bin/google-chrome",
  "/usr/bin/google-chrome-stable",
  "/usr/bin/chromium",
  "/usr/bin/chromium-browser",
].filter(Boolean)
const chromePath = chromeCandidates.find((candidate) => existsSync(candidate))

if (!chromePath) {
  throw new Error(`No Chrome/Chromium binary found. Checked: ${chromeCandidates.join(", ")}`)
}

const profileDir = `/tmp/zoo-park-view-proof-${process.pid}`
rmSync(profileDir, {recursive: true, force: true})

const chrome = spawn(
  chromePath,
  [
    "--headless=new",
    "--no-sandbox",
    "--disable-gpu",
    `--remote-debugging-port=${debuggingPort}`,
    `--user-data-dir=${profileDir}`,
    "--window-size=1440,900",
    "--use-angle=swiftshader",
    "--enable-unsafe-swiftshader",
    contentAcceptance ? "about:blank" : previewUrl,
  ],
  {stdio: "ignore"},
)

const sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds))

async function waitForPageTarget() {
  let lastError = null
  for (let attempt = 0; attempt < 60; attempt += 1) {
    try {
      const response = await fetch(`http://127.0.0.1:${debuggingPort}/json`)
      if (response.ok) {
        const targets = await response.json()
        const target = targets.find(
          (candidate) => candidate.type === "page",
        )
        if (target?.webSocketDebuggerUrl) return target
      }
    } catch (error) {
      lastError = error
    }
    await sleep(250)
  }
  throw new Error(`Chrome did not expose the Zoo page target: ${lastError ?? "timed out"}`)
}

function connectCdp(webSocketDebuggerUrl) {
  const socket = new WebSocket(webSocketDebuggerUrl)
  const pending = new Map()
  let nextId = 1
  let eventHandler = () => {}

  const opened = new Promise((resolve, reject) => {
    socket.addEventListener("open", resolve, {once: true})
    socket.addEventListener("error", reject, {once: true})
  })

  socket.addEventListener("message", (event) => {
    const message = JSON.parse(String(event.data))
    if (!message.id) {
      eventHandler(message)
      return
    }
    const request = pending.get(message.id)
    if (!request) return
    pending.delete(message.id)
    if (message.error) request.reject(new Error(`${message.error.code}: ${message.error.message}`))
    else request.resolve(message.result)
  })

  return {
    opened,
    onEvent(handler) { eventHandler = handler },
    close: () => socket.close(),
    send(method, params = {}) {
      const id = nextId
      nextId += 1
      return new Promise((resolve, reject) => {
        pending.set(id, {resolve, reject})
        socket.send(JSON.stringify({id, method, params}))
      })
    },
  }
}

// Browser dogfood for the 3D park view. The park is a single shared-renderer canvas, so the
// script drives real pointer, wheel, keyboard and touch input at projected world positions and
// reads camera state through the view's debug hook (window.__zooParkView).

let cdp = null
try {
  const target = await waitForPageTarget()
  cdp = connectCdp(target.webSocketDebuggerUrl)
  await cdp.opened
  await cdp.send("Page.enable")
  await cdp.send("Runtime.enable")
  let catalogRequests = 0
  let catalogFailure = null
  if (contentAcceptance) {
    // Real production catalog HTTP boundary; real Rust/WASM validates this data.
    // No window state, fake WASM, generated art or command result is injected.
    const catalogUrl = new URL(process.env.ZOO_CONTENT_CATALOG_PATH ?? "content/catalog.json", previewUrl).href
    const response = await fetch(catalogUrl)
    if (!response.ok) throw new Error(`Production catalog bootstrap missing: ${response.status} ${catalogUrl}`)
    const definition = await response.json()
    const food = definition.facilities.find((item) => item.key === "food")
    const zebra = definition.species.find((item) => item.key === "zebra")
    if (!food || !zebra) throw new Error("Real baseline catalog missing food/zebra")
    definition.facilities.push({...food, key: "acceptance_food_facility", label: "Acceptance catalog stand", build_cost_cents: 12300, price_cents: 456})
    definition.species.push({...zebra, key: "acceptance_species", label: "Acceptance catalog animal"})
    if (contentAcceptance === "invalid") definition.facilities.at(-1).asset_ref = "missing-real-asset"
    const body = Buffer.from(JSON.stringify(definition)).toString("base64")
    cdp.onEvent((event) => {
      if (event.method !== "Fetch.requestPaused") return
      catalogRequests += 1
      cdp.send("Fetch.fulfillRequest", {requestId: event.params.requestId, responseCode: 200, responseHeaders: [{name: "Content-Type", value: "application/json"}], body})
        .catch((error) => { catalogFailure = error })
    })
    await cdp.send("Fetch.enable", {patterns: [{urlPattern: catalogUrl, requestStage: "Request"}]})
  }
  await cdp.send("Page.navigate", {url: previewUrl})
  mkdirSync("test-results", {recursive: true})

  const evaluate = async (expression) => {
    const response = await cdp.send("Runtime.evaluate", {
      expression,
      awaitPromise: true,
      returnByValue: true,
    })
    if (response.exceptionDetails) {
      throw new Error(response.exceptionDetails.exception?.description ?? response.exceptionDetails.text)
    }
    return response.result.value
  }

  const waitFor = async (description, expression, attempts = 80) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await evaluate(expression)) return
      await sleep(100)
    }
    const state = await evaluate(`({
      url: location.href,
      renderer: document.querySelector('.park-view')?.dataset.renderer ?? null,
      hook: Boolean(window.__zooParkView),
      nodes: window.__zooParkView?.nodeCount?.() ?? null,
      message: document.querySelector('.message')?.textContent ?? null,
    })`)
    throw new Error(`Timed out waiting for ${description}: ${JSON.stringify(state)}`)
  }

  const assert = (condition, message, details) => {
    if (!condition) throw new Error(`${message}${details === undefined ? "" : `: ${JSON.stringify(details)}`}`)
  }

  const camera = () => evaluate(`window.__zooParkView.camera()`)
  const footprintInFrame = () => evaluate(`window.__zooParkView.parkFootprintInFrame()`)
  const project = (point) => evaluate(`window.__zooParkView.projectWorld(${JSON.stringify(point)})`)
  const tile = (x, y) => project([x + 0.5, 0, y + 0.5])
  const message = () => evaluate(`document.querySelector('.message')?.textContent ?? ''`)
  const frame = () => sleep(120)

  const mouse = (type, point, extra = {}) =>
    cdp.send("Input.dispatchMouseEvent", {type, x: point.x, y: point.y, button: "none", ...extra})
  const click = async (point) => {
    await mouse("mouseMoved", point)
    await mouse("mousePressed", point, {button: "left", buttons: 1, clickCount: 1})
    await mouse("mouseReleased", point, {button: "left", buttons: 0, clickCount: 1})
    await frame()
  }
  const drag = async (from, to, button = "left") => {
    const buttons = button === "left" ? 1 : button === "right" ? 2 : 4
    await mouse("mouseMoved", from)
    await mouse("mousePressed", from, {button, buttons, clickCount: 1})
    for (let step = 1; step <= 12; step += 1) {
      const t = step / 12
      await mouse("mouseMoved", {x: from.x + (to.x - from.x) * t, y: from.y + (to.y - from.y) * t}, {button, buttons})
    }
    await mouse("mouseReleased", to, {button, buttons: 0, clickCount: 1})
    await frame()
  }
  const wheel = (point, deltaY) =>
    cdp.send("Input.dispatchMouseEvent", {type: "mouseWheel", x: point.x, y: point.y, deltaX: 0, deltaY})
  const key = async (value, times = 1) => {
    for (let index = 0; index < times; index += 1) {
      const code = value.length === 1 ? `Key${value.toUpperCase()}` : value
      await cdp.send("Input.dispatchKeyEvent", {type: "keyDown", key: value, code, text: value.length === 1 ? value : undefined})
      await cdp.send("Input.dispatchKeyEvent", {type: "keyUp", key: value, code})
    }
    await frame()
  }
  const touch = (type, points) =>
    cdp.send("Input.dispatchTouchEvent", {type, touchPoints: points.map(([x, y], id) => ({x, y, id}))})
  const clickButton = (label) =>
    evaluate(`(() => {
      const button = [...document.querySelectorAll('button')].find((candidate) =>
        (candidate.getAttribute('aria-label') ?? candidate.textContent ?? '').includes(${JSON.stringify(label)}))
      if (!button || button.disabled) return false
      button.click()
      return true
    })()`)
  const closeInspector = () =>
    evaluate(`(() => {
      const button = document.querySelector('.window-title button')
      button?.click()
      return Boolean(button)
    })()`)
  const screenshot = async (path) => {
    const shot = await cdp.send("Page.captureScreenshot", {format: "png", fromSurface: true})
    writeFileSync(path, Buffer.from(shot.data, "base64"))
  }

  // Camera invariant: at zoom 1 the whole park stays in frame through a full orbit at both
  // tilt bounds. Keyboard orbit/tilt drive the same zoo-scene rig as mouse and touch gestures.
  const assertFullOrbitAtTiltBounds = async (label) => {
    await key("Home")
    for (const [tiltKey, bound] of [["r", "upper"], ["f", "lower"]]) {
      await key(tiltKey, 20)
      const start = await camera()
      for (let step = 0; step < 24; step += 1) {
        assert(await footprintInFrame(), `${label}: park left the frame at ${bound} tilt`, await camera())
        await key("e")
      }
      const end = await camera()
      assert(Math.abs(end.yawDegrees - start.yawDegrees) < 0.01, `${label}: full orbit did not return`, {start, end})
    }
    const bounds = []
    await key("r", 20)
    bounds.push((await camera()).pitchDegrees)
    await key("f", 30)
    bounds.push((await camera()).pitchDegrees)
    await key("f", 5)
    assert((await camera()).pitchDegrees === bounds[1], `${label}: lower tilt bound not clamped`, bounds)
    await key("Home")
  }

  if (contentAcceptance === "invalid") {
    await waitFor("Rust catalog asset_ref rejection", `document.body.textContent.includes('asset_ref')`)
    assert(catalogRequests > 0 && !catalogFailure, "Invalid fixture did not cross the real catalog boundary")
    assert(!(await evaluate(`window.__zooParkView?.ready === true`)), "Invalid catalog silently booted a fallback park")
    console.log("catalog browser rejection passed: actual invalid asset reference, no fallback park")
    process.exitCode = 0
  } else {
  // --- Desktop ---------------------------------------------------------------------------------
  await waitFor("the shared 3D park renderer", `Boolean(
    window.__zooParkView?.ready &&
    window.__zooParkView.nodeCount() > 500 &&
    document.querySelector('.park-view[data-renderer="ready"] canvas.park-canvas')
  )`)
  assert(await evaluate(`document.querySelectorAll('.park-view button, .park-view .tile').length === 0`),
    "Park view must not contain DOM game objects")
  const initial = await camera()
  assert(await footprintInFrame(), "Default view does not frame the whole park", initial)
  await assertFullOrbitAtTiltBounds("desktop")

  const center = await tile(10, 7)
  await wheel(center, -600)
  await frame()
  assert((await camera()).zoom > 1.5, "Wheel did not zoom in", await camera())
  const beforeOrbit = await camera()
  await drag(center, {x: center.x + 200, y: center.y - 60}, "right")
  const afterOrbit = await camera()
  assert(afterOrbit.yawDegrees !== beforeOrbit.yawDegrees && afterOrbit.pitchDegrees !== beforeOrbit.pitchDegrees,
    "Right-drag did not orbit and tilt", {beforeOrbit, afterOrbit})
  await drag(center, {x: center.x - 150, y: center.y + 90})
  const afterPan = await camera()
  assert(afterPan.target[0] !== afterOrbit.target[0] || afterPan.target[2] !== afterOrbit.target[2],
    "Left-drag did not pan", {afterOrbit, afterPan})
  assert(await clickButton("Reset view"), "Reset view button missing")
  await frame()
  const reset = await camera()
  assert(reset.yawDegrees === initial.yawDegrees && reset.zoom === 1 && reset.target.join() === initial.target.join(),
    "Reset view did not restore the default camera", {initial, reset})

  // Gameplay loop through canvas picking.
  assert(await clickButton("Path"), "Path tool missing")
  await drag(await tile(3, 7), await tile(12, 7))
  await drag(await tile(8, 7), await tile(8, 6))
  assert((await message()).includes("Path"), "Path drag did not build", await message())
  assert(await clickButton("Draw habitat"), "Habitat tool missing")
  await drag(await tile(6, 1), await tile(11, 5))
  assert((await message()).includes("Habitat #1 fenced"), "Habitat drag did not fence", await message())
  // A rejected fence keeps its candidate footprint in red, and releasing it builds nothing.
  assert(await clickButton("Ⅱ"), "Pause control missing")
  await frame()
  const stats = () => evaluate(`[...document.querySelectorAll('.stat strong')].map((node) => node.textContent).join('|')`)
  const statsBeforeRejected = await stats()
  const nodesBeforeRejected = await evaluate(`window.__zooParkView.nodeCount()`)
  const rejectedFrom = await tile(3, 5)
  const rejectedTo = await tile(5, 8)
  await mouse("mouseMoved", rejectedFrom)
  await mouse("mousePressed", rejectedFrom, {button: "left", buttons: 1, clickCount: 1})
  await mouse("mouseMoved", rejectedTo, {button: "left", buttons: 1})
  await waitFor("the rejected placement explanation",
    `document.querySelector('.invalid')?.textContent.includes('clear grass') ?? false`)
  await frame()
  const rejectedPreviewNodes = (await evaluate(`window.__zooParkView.nodeCount()`)) - nodesBeforeRejected
  // 12 ghost tiles plus two rails per fence segment (14) and posts; without fence geometry only
  // the ghost tiles would be added.
  assert(rejectedPreviewNodes >= 12 + 14 * 2, "Rejected placement lost its fence preview", {rejectedPreviewNodes})
  await screenshot("test-results/park-view-rejected-placement.png")
  await mouse("mouseReleased", rejectedTo, {button: "left", buttons: 0, clickCount: 1})
  await frame()
  assert((await message()).includes("clear grass"), "Rejected fence release was not explained", await message())
  assert((await stats()) === statsBeforeRejected, "Rejected fence release changed the park",
    {statsBeforeRejected, after: await stats()})
  assert(await clickButton("1×"), "Resume control missing")
  assert(await clickButton("Food stand"), "Food tool missing")
  await click(await tile(5, 8))
  assert((await message()).includes("Food stand #1 built"), "Food stand was not placed", await message())
  assert(await clickButton("Drink stand"), "Drink tool missing")
  await click(await tile(10, 8))
  assert((await message()).includes("Drink stand #2 built"), "Drink stand was not placed", await message())
  if (contentAcceptance) {
    assert(await clickButton("Ⅱ"), "Pause before exact catalog cost proof missing")
    const displayedCash = async () => Number((await evaluate(`document.querySelector('.stat strong')?.textContent ?? ''`)).replace(/[^0-9]/g, ""))
    const before = await displayedCash()
    assert(await clickButton("Acceptance catalog stand"), "Definition-only facility was not rendered as an actual tool")
    await click(await tile(12, 6))
    assert((await message()).includes("Acceptance catalog stand #3 built"), "Definition-only facility did not reach Rust placement", await message())
    assert(before - await displayedCash() === 123, "Actual authoritative canary construction cost mismatch")
    await click(await tile(12, 6))
    assert(before - await displayedCash() === 123, "Repeated placement charged a second construction cost")
    assert(catalogRequests > 0 && !catalogFailure, "Canary never crossed actual production catalog loader")
    await screenshot("test-results/catalog-definition-only-facility.png")
  }
  assert(await clickButton("Inspect"), "Inspect tool missing")
  await click(await project([2.5, 0.5, 6.5]))
  await waitFor("the depot inspector", `document.querySelector('.window-title span')?.textContent === 'Central operations depot'`)
  for (const label of ["feed crates", "Hire keeper", "Hire janitor", "Hire mechanic"]) {
    assert(await clickButton(label), `Depot action missing: ${label}`)
  }
  await click(await tile(8, 3))
  await waitFor("the habitat inspector", `document.querySelector('.window-title span')?.textContent === 'Habitat #1'`)
  assert(await clickButton("Schedule available keeper"), "Keeper scheduling missing")
  assert(await clickButton("Add basic shelter"), "Shelter action missing")
  for (let index = 0; index < 3; index += 1) assert(await clickButton(contentAcceptance ? "Acceptance catalog animal" : "Zebra"), "Catalog adoption unavailable")
  assert((await message()).includes(contentAcceptance ? "Acceptance catalog animal adopted" : "Zebra adopted"), "Catalog species were not adopted through Rust", await message())
  assert(await clickButton("4×"), "Speed control missing")
  await waitFor("guests to arrive", `Number(document.querySelectorAll('.stat strong')[1]?.textContent) > 0`, 200)
  await sleep(1500)
  await screenshot("test-results/park-view-desktop.png")

  // --- Phone-sized touch viewport ----------------------------------------------------------------
  await cdp.send("Emulation.setDeviceMetricsOverride", {width: 390, height: 844, deviceScaleFactor: 2, mobile: true})
  await cdp.send("Emulation.setTouchEmulationEnabled", {enabled: true, maxTouchPoints: 5})
  await closeInspector()
  await assertFullOrbitAtTiltBounds("phone")
  await clickButton("Ⅱ")
  const phoneCenter = await evaluate(`(() => {
    const rect = document.querySelector('.park-canvas').getBoundingClientRect()
    return {x: rect.left + rect.width / 2, y: rect.top + rect.height / 2}
  })()`)
  const pinchStart = await camera()
  await touch("touchStart", [[phoneCenter.x - 40, phoneCenter.y], [phoneCenter.x + 40, phoneCenter.y]])
  for (let step = 1; step <= 12; step += 1) {
    const radius = 40 + step * 6
    const angle = step * 0.05
    await touch("touchMove", [
      [phoneCenter.x - radius * Math.cos(angle), phoneCenter.y - radius * Math.sin(angle)],
      [phoneCenter.x + radius * Math.cos(angle), phoneCenter.y + radius * Math.sin(angle)],
    ])
  }
  await touch("touchEnd", [])
  await frame()
  const pinchEnd = await camera()
  assert(pinchEnd.zoom > pinchStart.zoom * 1.5 && pinchEnd.yawDegrees !== pinchStart.yawDegrees,
    "Two-finger pinch/twist did not zoom and rotate", {pinchStart, pinchEnd})
  await touch("touchStart", [[phoneCenter.x - 50, phoneCenter.y], [phoneCenter.x + 50, phoneCenter.y]])
  for (let step = 1; step <= 10; step += 1) {
    await touch("touchMove", [[phoneCenter.x - 50, phoneCenter.y - step * 12], [phoneCenter.x + 50, phoneCenter.y - step * 12]])
  }
  await touch("touchEnd", [])
  await frame()
  const tiltEnd = await camera()
  assert(tiltEnd.pitchDegrees < pinchEnd.pitchDegrees && Math.abs(tiltEnd.zoom - pinchEnd.zoom) < 0.01,
    "Two-finger vertical drag did not tilt", {pinchEnd, tiltEnd})
  const panStart = await camera()
  await touch("touchStart", [[phoneCenter.x, phoneCenter.y]])
  for (let step = 1; step <= 8; step += 1) await touch("touchMove", [[phoneCenter.x + step * 10, phoneCenter.y + step * 6]])
  await touch("touchEnd", [])
  await frame()
  const panEnd = await camera()
  assert(panEnd.target.join() !== panStart.target.join(), "One-finger drag did not pan", {panStart, panEnd})
  await key("Home")
  assert(await footprintInFrame(), "Phone reset does not frame the whole park")
  // With the Path tool active, the first finger of a two-finger camera gesture must not place
  // (and pay for) a path; a single-finger tap on the same tile still does.
  const cash = () => evaluate(`document.querySelector('.stat strong')?.textContent ?? ''`)
  assert(await clickButton("Path"), "Path tool missing on phone")
  const pathTile = await tile(13, 7)
  const cashBeforePinch = await cash()
  await touch("touchStart", [[pathTile.x, pathTile.y]])
  await frame()
  await touch("touchStart", [[pathTile.x, pathTile.y], [pathTile.x + 80, pathTile.y]])
  for (let step = 1; step <= 8; step += 1) {
    await touch("touchMove", [[pathTile.x - step * 4, pathTile.y], [pathTile.x + 80 + step * 4, pathTile.y]])
  }
  await touch("touchEnd", [])
  await frame()
  assert((await cash()) === cashBeforePinch, "Two-finger gesture with the Path tool placed a path",
    {cashBeforePinch, cashAfter: await cash()})
  await key("Home")
  const tapTile = await tile(13, 7)
  await touch("touchStart", [[tapTile.x, tapTile.y]])
  await touch("touchEnd", [])
  await frame()
  assert((await cash()) !== cashBeforePinch, "Single-finger tap with the Path tool did not place a path",
    {cashBeforePinch, message: await message()})
  assert(await clickButton("Inspect"), "Inspect tool missing on phone")
  await key("Home")
  const depot = await project([2.5, 0.5, 6.5])
  await touch("touchStart", [[depot.x, depot.y]])
  await touch("touchEnd", [])
  await waitFor("the depot inspector after a tap", `document.querySelector('.window-title span')?.textContent === 'Central operations depot'`)
  await closeInspector()
  await frame()
  await screenshot("test-results/park-view-phone.png")

  console.log(
    "Park view dogfood passed: shared-renderer 3D park, full orbit at both tilt bounds (desktop + phone), wheel/drag/keyboard/touch camera control, canvas-picked path/habitat/stand/depot/habitat commands, and visual proofs.",
  )
  }
} finally {
  cdp?.close()
  chrome.kill("SIGTERM")
  rmSync(profileDir, {recursive: true, force: true})
}
