// #98 park circulation browser proof. Called by capture-park-view-proof.mjs in the same production
// preview and headless Chromium. On a fresh park it paints a public loop, widens one side with a
// two-tile brush, lays a separate staff-only service route from the depot and puts a food stand
// between the two. It then breaks the public connection to the entrance (the access overlay marks
// the cut-off loop while the staff route stays connected, and the stand switches to its staff-only
// service entrance) and repairs it. Last, on a phone-sized touch viewport, it taps one staff-only
// tile. All prices, counts and service entrances are read from Rust's messages and snapshot views.
import { enterNewPark } from "./park-library-proof.mjs"

export async function runCirculationAcceptance(context) {
  const { cdp, evaluate, assert, sleep, previewUrl, tile, clickButton, mouse, key, frame, screenshot } =
    context

  const message = () => evaluate(`document.querySelector('.message')?.textContent ?? ''`)
  const waitFor = async (description, expression, attempts = 100) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await evaluate(expression)) return
      await sleep(100)
    }
    throw new Error(`#98 acceptance timed out waiting for ${description}: ${await message()}`)
  }
  const expectMessage = (text) =>
    waitFor(
      `the message "${text}"`,
      `document.querySelector('.message')?.textContent.includes(${JSON.stringify(text)}) ?? false`,
    )
  const pressData = async (selector) => {
    const pressed = await evaluate(`(() => {
      const button = document.querySelector(${JSON.stringify(selector)})
      if (!button) return false
      button.click()
      return true
    })()`)
    assert(pressed, `#98: control ${selector} missing`)
    await frame()
  }
  const brush = async ({ access, width, plaza = false }) => {
    await pressData(`[data-path-access="${access}"]`)
    await pressData(plaza ? `[data-path-shape="plaza"]` : `[data-path-width="${width}"]`)
  }
  const press = (point) => mouse("mousePressed", point, { button: "left", buttons: 1, clickCount: 1 })
  const release = async (point) => {
    await mouse("mouseReleased", point, { button: "left", buttons: 0, clickCount: 1 })
    await frame()
  }
  /** One held gesture through every corner of `corners`, released on the last. */
  const paint = async (corners) => {
    const points = []
    for (const [x, y] of corners) points.push(await tile(x, y))
    await mouse("mouseMoved", points[0])
    await press(points[0])
    for (let index = 1; index < points.length; index += 1) {
      const [a, b] = [points[index - 1], points[index]]
      for (let step = 1; step <= 8; step += 1) {
        const t = step / 8
        await mouse("mouseMoved", { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t }, { button: "left", buttons: 1 })
      }
    }
    await frame()
    await release(points.at(-1))
    await sleep(150)
  }
  const tap = async (x, y) => paint([[x, y]])
  const node = (x, y) => evaluate(`window.__zooParkView.staticNode("tile:${x}:${y}")`)
  const setOverlay = async (kind) => {
    await evaluate(`(() => {
      const select = document.querySelector('select[aria-label="Overlay"]')
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(select, ${JSON.stringify(kind)})
      select.dispatchEvent(new Event('change', { bubbles: true }))
    })()`)
    await frame()
  }
  const legendCount = (level) =>
    evaluate(`(() => {
      const swatch = document.querySelector('.overlay-legend .legend-swatch.${level}')
      return Number(swatch?.parentElement?.querySelector('strong')?.textContent ?? -1)
    })()`)
  const serviceText = () => evaluate(`document.querySelector('[data-stand-service]')?.textContent ?? ''`)
  const settleCamera = async () => {
    let last = ""
    for (let attempt = 0; attempt < 60; attempt += 1) {
      const now = JSON.stringify(await evaluate("window.__zooParkView.camera()"))
      if (now === last) return
      last = now
      await sleep(150)
    }
  }

  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  await cdp.send("Page.navigate", { url: previewUrl })
  assert((await enterNewPark({ evaluate, sleep })) === "started", "#98: a fresh park did not start")
  await waitFor("the park renderer", `Boolean(window.__zooParkView?.ready && window.__zooParkView.nodeCount() > 500)`, 200)
  assert(await clickButton("Ⅱ"), "#98: pause control missing")
  await key("Home")
  await settleCamera()

  // --- A public loop, widened on one side ---------------------------------------------------
  // The fresh park has an entrance at 0,7, the depot at 2,6 and a public path from 1,7 to 4,7.
  assert(await clickButton("Path"), "#98: path tool missing")
  await brush({ access: "public", width: 1 })
  await paint([[5, 7], [12, 7], [12, 12], [5, 12], [5, 8]])
  await expectMessage("Path built · 24 new public path tiles")
  await brush({ access: "public", width: 2 })
  await paint([[5, 7], [8, 7]])
  // A 5×2 band along the existing top row adds only its four new tiles, each priced.
  await expectMessage("Path built · 4 new public path tiles")

  // --- A separate staff-only service route ---------------------------------------------------
  await brush({ access: "staff", width: 1 })
  await paint([[3, 6], [3, 3], [14, 3], [14, 9], [12, 9]])
  await expectMessage("new staff-only path tile")
  await expectMessage("1 tile becomes staff-only (free)")
  const staff = await node(3, 4)
  const sand = await node(6, 7)
  assert(staff && sand && staff.color !== sand.color, "#98: staff-only paths look like public paths", { staff, sand })

  // A stand between the networks: guests buy from the loop, mechanics may use either side.
  const placed = await evaluate(`(() => {
    const button = [...document.querySelectorAll('.toolbar button')].find((b) => (b.getAttribute('aria-label') ?? b.textContent).includes('Food stand'))
    if (!button) return false
    button.click()
    return true
  })()`)
  assert(placed, "#98: food stand tool missing")
  await frame()
  await tap(13, 8)
  await expectMessage("Food stand #1 built")
  assert(await clickButton("Inspect"), "#98: inspect tool missing")
  await tap(13, 8)
  await waitFor("the stand inspector", `Boolean(document.querySelector('[data-stand-service]'))`)
  const before = await serviceText()
  assert(before.includes("via public path") && before.includes("alternatives"), "#98: the stand does not show its service entrance and alternatives", before)
  await setOverlay("access")
  assert((await legendCount("bad")) === 0, "#98: the connected park shows cut-off paths", await legendCount("bad"))
  await screenshot("test-results/circulation-desktop.png")

  // --- Break the public connection, then repair it -------------------------------------------
  assert(await clickButton("Demolish"), "#98: demolish tool missing")
  await tap(4, 7)
  await expectMessage("Path removed")
  await waitFor("the cut-off loop in the access overlay", `(() => {
    const swatch = document.querySelector('.overlay-legend .legend-swatch.bad')
    return Number(swatch?.parentElement?.querySelector('strong')?.textContent ?? 0) > 0
  })()`)
  const legend = await evaluate(`document.querySelector('.overlay-legend')?.textContent ?? ''`)
  assert(legend.includes("public path tile(s) are not connected to the entrance"), "#98: the overlay does not name the cut-off public paths", legend)
  assert(!legend.includes("staff-only path tile(s) are not connected"), "#98: the staff route lost its depot connection", legend)
  assert(await clickButton("Inspect"), "#98: inspect tool missing")
  await tap(13, 8)
  await waitFor("the staff-only service entrance", `document.querySelector('[data-stand-service]')?.textContent.includes('via staff-only path') ?? false`)
  await screenshot("test-results/circulation-broken.png")

  assert(await clickButton("Path"), "#98: path tool missing")
  await brush({ access: "public", width: 1 })
  await tap(4, 7)
  await expectMessage("Path built · 1 new public path tile")
  await waitFor("a fully connected access overlay", `(() => {
    const swatch = document.querySelector('.overlay-legend .legend-swatch.bad')
    return Number(swatch?.parentElement?.querySelector('strong')?.textContent ?? -1) === 0
  })()`)
  assert(await clickButton("Inspect"), "#98: inspect tool missing")
  await tap(13, 8)
  await waitFor("the public service entrance again", `document.querySelector('[data-stand-service]')?.textContent.includes('via public path') ?? false`)
  await setOverlay("")

  // --- Phone: a touch tap lays one staff-only tile --------------------------------------------
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 2, mobile: true })
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 })
  await sleep(600)
  await evaluate(`document.querySelector('.window-title button')?.click()`)
  // Fold the guide checklist to its one-line summary so the map is tappable.
  await evaluate(`(() => {
    const guide = document.querySelector('details[aria-label="Park guide"]')
    if (guide?.open) guide.querySelector('summary').click()
  })()`)
  await key("Home")
  await settleCamera()
  assert(await clickButton("Path"), "#98: path tool missing on phone")
  await brush({ access: "staff", width: 1 })
  const target = await tile(9, 5)
  const onCanvas = await evaluate(`document.elementFromPoint(${target.x}, ${target.y})?.tagName === 'CANVAS'`)
  assert(target.visible && onCanvas, "#98: the phone tile is not on the park canvas", { target, hit: await evaluate(`document.elementFromPoint(${target.x}, ${target.y})?.outerHTML.slice(0, 160) ?? null`) })
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: target.x, y: target.y, id: 0 }] })
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] })
  await frame()
  await expectMessage("Path built · 1 new staff-only path tile")
  await screenshot("test-results/circulation-phone.png")
  // Later proofs expect the default brush.
  await brush({ access: "public", width: 1 })
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })

  console.log(
    "Circulation acceptance passed: painted a public loop, widened it with a two-tile brush at the per-tile price, laid a separate staff-only service route (reclassifying one tile for free), saw a stand's service entrance and alternatives, broke the public connection (overlay marked the cut-off loop while the staff route stayed connected and the stand switched to its staff-only entrance), repaired it, and laid a staff-only tile by touch on a phone.",
  )
}
