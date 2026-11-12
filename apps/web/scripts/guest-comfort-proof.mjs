// #101 guest-comfort browser proof. Called by capture-park-view-proof.mjs in the same production
// preview and headless Chromium. In the Riverside zoo, which opens with guests and stands but no
// toilets, the player runs time until zoo-core reports guests who need a toilet and found none
// (the alert and the guests' complaint), uses the alert's own action to pick the toilet tool,
// builds toilets beside the main path by real canvas picking, opens their inspector and watches a
// guest occupy a cubicle, then sees the complaint resolve: the alert disappears and the inspector
// reports no unmet demand. Last, on a phone-sized touch viewport, a bench is placed by touch.
// Every number and state is read from the DOM zoo-core's snapshot renders.
import { enterNewPark } from "./park-library-proof.mjs"

const WORKSPACE = `section[aria-label="Management workspace"]`
const TOILET = { x: 12, y: 9 }
const BENCH = { x: 10, y: 9 }

export async function runGuestComfortAcceptance(context) {
  const { cdp, evaluate, assert, sleep, previewUrl, tile, clickButton, mouse, key, frame, screenshot } =
    context

  const message = () => evaluate(`document.querySelector('.message')?.textContent ?? ''`)
  const waitFor = async (description, expression, attempts = 150) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await evaluate(expression)) return
      await sleep(100)
    }
    const state = await evaluate(`({
      alerts: [...document.querySelectorAll('.alert-item')].map((item) => item.dataset.code),
      message: document.querySelector('.message')?.textContent ?? null,
      panel: document.querySelector('.side-panel .window-title span')?.textContent ?? null,
      status: document.querySelector('[data-comfort-status]')?.textContent ?? null,
    })`)
    throw new Error(`#101 acceptance timed out waiting for ${description}: ${JSON.stringify(state)}`)
  }
  const alertShown = (code) =>
    `[...document.querySelectorAll('.alert-item')].some((item) => item.dataset.code === ${JSON.stringify(code)})`
  const openAlerts = async () => {
    if (!(await evaluate(`Boolean(document.querySelector('${WORKSPACE}'))`))) {
      assert(await clickButton("Management workspace"), "#101: Manage control missing")
      await waitFor("the workspace", `Boolean(document.querySelector('${WORKSPACE}'))`, 30)
    }
    await evaluate(`[...document.querySelectorAll('${WORKSPACE} [role="tab"]')].find((b) => b.textContent.startsWith('Alerts'))?.click()`)
    await frame()
  }
  const settleCamera = async () => {
    let last = ""
    for (let attempt = 0; attempt < 60; attempt += 1) {
      const now = JSON.stringify(await evaluate("window.__zooParkView.camera()"))
      if (now === last) return
      last = now
      await sleep(150)
    }
  }
  const pickTile = async (label, at) => {
    await key("Home")
    await settleCamera()
    const point = await tile(at.x, at.y)
    assert(point.visible, `#101: ${label} is off screen`, point)
    assert(
      await evaluate(`document.elementFromPoint(${point.x}, ${point.y})?.classList.contains('park-canvas') ?? false`),
      `#101: the HUD covers ${label}`,
    )
    return point
  }

  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  await cdp.send("Page.navigate", { url: previewUrl })
  assert(
    (await enterNewPark({ evaluate, sleep }, { scenario: "living-starter" })) === "started",
    "#101: the Riverside zoo did not start",
  )
  await waitFor("the park renderer", `Boolean(window.__zooParkView?.ready && window.__zooParkView.nodeCount() > 500)`, 200)

  // --- An unmet need --------------------------------------------------------------------------
  assert(await clickButton("4×"), "#101: speed control missing")
  await openAlerts()
  await waitFor("guests who need a toilet", alertShown("toilet_demand_unmet"), 900)
  assert(await clickButton("Ⅱ"), "#101: pause control missing")
  const unmet = await evaluate(
    `document.querySelector('.alert-item[data-code="toilet_demand_unmet"] p')?.textContent ?? ''`,
  )
  assert(/needs? a toilet/.test(unmet), "#101: the alert does not name the need", unmet)
  await screenshot("test-results/guest-comfort-unmet.png")

  // --- The alert picks the toilet tool; the player builds by the main path ---------------------
  const pressed = await evaluate(`(() => {
    const item = document.querySelector('${WORKSPACE} .alert-item[data-code="toilet_demand_unmet"]')
    const button = [...(item?.querySelectorAll('button') ?? [])].find((b) => b.textContent.startsWith('Build Toilets'))
    button?.click()
    return Boolean(button)
  })()`)
  assert(pressed, "#101: the alert offers no toilet to build")
  await frame()
  assert(
    await evaluate(`document.querySelector(".toolbar .tool.active")?.textContent.includes("Toilets") ?? false`),
    "#101: the alert action did not pick the toilet tool",
    await evaluate(`document.querySelector(".toolbar .tool.active")?.textContent ?? null`),
  )
  const toiletPoint = await pickTile("the toilet plot", TOILET)
  await mouse("mouseMoved", toiletPoint)
  await mouse("mousePressed", toiletPoint, { button: "left", buttons: 1, clickCount: 1 })
  await mouse("mouseReleased", toiletPoint, { button: "left", buttons: 0, clickCount: 1 })
  await frame()
  assert(/Toilets #\d+ built/.test(await message()), "#101: the toilets were not built", await message())

  // --- Inspect the toilets and watch a guest use them --------------------------------------------
  await openAlerts()
  await evaluate(`[...document.querySelectorAll('${WORKSPACE} [role="tab"]')].find((b) => b.textContent.startsWith('Facilities'))?.click()`)
  await frame()
  assert(
    await evaluate(`(() => {
      const row = [...document.querySelectorAll('${WORKSPACE} tbody tr')].find((r) => r.textContent.startsWith('Toilets #'))
      row?.click()
      return Boolean(row)
    })()`),
    "#101: the facilities table does not list the toilets",
  )
  await frame()
  await evaluate(`document.querySelector('${WORKSPACE} .window-title button')?.click()`)
  await frame()
  await waitFor(
    "the toilet inspector",
    `(document.querySelector('.side-panel .window-title span')?.textContent ?? '').startsWith('Toilets #')`,
    30,
  )
  assert(await clickButton("4×"), "#101: speed control missing")
  await waitFor(
    "a guest using the toilets",
    `/Open · [1-9]\\/\\d+ in use/.test(document.querySelector('[data-comfort-status]')?.textContent ?? '')`,
    900,
  )
  assert(await clickButton("Ⅱ"), "#101: pause control missing")
  await screenshot("test-results/guest-comfort-in-use.png")
  assert(await clickButton("4×"), "#101: speed control missing")
  await waitFor(
    "the complaint to resolve",
    `Number(document.querySelector('[data-comfort-uses]')?.textContent ?? 0) > 0 &&
      document.querySelector('[data-comfort-unmet]')?.textContent === 'None' &&
      !${alertShown("toilet_demand_unmet")}`,
    900,
  )
  assert(await clickButton("Ⅱ"), "#101: pause control missing")
  await screenshot("test-results/guest-comfort-resolved.png")

  // --- Phone: place a bench by touch -----------------------------------------------------------
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 2, mobile: true })
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 })
  await sleep(400)
  await evaluate(`document.querySelector('.side-panel .window-title button')?.click()`)
  await frame()
  assert(await clickButton("Bench ·"), "#101: bench tool missing")
  const benchPoint = await pickTile("the bench plot", BENCH)
  await cdp.send("Input.dispatchTouchEvent", {
    type: "touchStart",
    touchPoints: [{ x: benchPoint.x, y: benchPoint.y, id: 0 }],
  })
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] })
  await frame()
  assert(/Bench #\d+ built/.test(await message()), "#101: the bench was not built by touch", await message())
  await screenshot("test-results/guest-comfort-phone.png")
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  console.log(
    "#101 guest-comfort acceptance passed: guests' unmet toilet need is alerted, the alert picks the toilet tool, toilets built by the path are used by a guest and the complaint resolves; a bench is placed by touch on a phone",
  )
}
