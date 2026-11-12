// #106 browser acceptance: the player changes a price, watches its effect over a comparable
// stretch of park time in the pricing report, and uses that report to correct it.
//
// Called by capture-park-view-proof.mjs against the same production preview and real headless
// Chromium. The player opens the authored two-habitat park (crates/zoo-core/tests/fixtures/
// pricing-park.json, written by zoo-core's own test) through the real file import; its food
// stand is overpriced. In Manage › Prices the player:
//   1. measures admissions at the default ticket over one window;
//   2. types a much dearer ticket and, over an equal window, sees zoo-core turn groups away as
//      "too dear" and admit fewer people;
//   3. reads zoo-core's forecast for candidate prices, sets the dearest one every recent group
//      would pay, and over an equal window sees nobody turned away for price again;
//   4. reads the stand's "too dear" complaints and its usual price from the same report, sets
//      that price, and over an equal window sees the complaints stop and sales rise.
// On a phone the panel is used by touch: the park is closed and reopened, and a draft price is
// cancelled with Escape. Park time and every figure come from zoo-core's snapshot.

import { readFileSync, writeFileSync } from "node:fs"

const MENU = `[aria-label="Park menu"][role="dialog"]`
const WORKSPACE = `section[aria-label="Management workspace"]`
const PANEL = `${WORKSPACE} .pricing-panel`
const FIXTURE = new URL("../../../crates/zoo-core/tests/fixtures/pricing-park.json", import.meta.url)
/** Park minutes each comparison window lasts. */
const WINDOW = 120
const DEAR_TICKET = "40"

export async function runPricingAcceptance(context) {
  const { cdp, evaluate, assert, sleep, screenshot, clickButton, frame } = context
  const waitFor = async (description, expression, attempts = 150) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await evaluate(expression)) return
      await sleep(100)
    }
    const state = await evaluate(`({
      message: document.querySelector('.message')?.textContent ?? null,
      panel: document.querySelector('${PANEL}')?.textContent ?? null,
    })`)
    throw new Error(`#106 acceptance timed out waiting for ${description}: ${JSON.stringify(state)}`)
  }
  const tick = () => evaluate(`JSON.parse(window.__zooDiagnostics.exportText()).session.clock.tick`)
  const openPrices = async () => {
    if (!(await evaluate(`Boolean(document.querySelector('${WORKSPACE}'))`))) {
      assert(await clickButton("Management workspace"), "#106: Manage control missing")
      await waitFor("the workspace", `Boolean(document.querySelector('${WORKSPACE}'))`, 30)
    }
    await evaluate(`[...document.querySelectorAll('${WORKSPACE} [role="tab"]')].find((b) => b.textContent.startsWith('Prices'))?.click()`)
    await waitFor("the pricing panel", `Boolean(document.querySelector('${PANEL}'))`, 30)
  }
  /** Today's gate and stand figures, as the pricing panel shows them. */
  const report = () =>
    evaluate(`(() => {
      const panel = document.querySelector('${PANEL}')
      const figures = panel.querySelector('.pricing-figures').dataset
      const stand = panel.querySelector('.stand-prices li').dataset
      return {
        admission: Number(panel.dataset.admissionPrice),
        open: panel.dataset.parkOpen === 'true',
        arrivals: Number(figures.gateArrivals),
        admitted: Number(figures.gateAdmitted),
        tooDear: Number(figures.gateTooExpensive),
        complaints: Number(figures.priceComplaints),
        standId: Number(stand.standId),
        standPrice: Number(stand.standPrice),
        standSold: Number(stand.standSold),
        standTooDear: Number(stand.standTooExpensive),
        standText: panel.querySelector('.stand-prices li small')?.textContent ?? '',
      }
    })()`)
  /** Runs `minutes` of park time at 4×; returns what changed in the report meanwhile. */
  const over = async (minutes) => {
    const start = await tick()
    const before = await report()
    assert(await clickButton("4×"), "#106: speed control missing")
    await waitFor(`${minutes} park minutes`, `JSON.parse(window.__zooDiagnostics.exportText()).session.clock.tick >= ${start + minutes}`, 1500)
    assert(await clickButton("Ⅱ"), "#106: pause control missing")
    await frame()
    const after = await report()
    const delta = {}
    for (const key of ["arrivals", "admitted", "tooDear", "standSold", "standTooDear"]) {
      delta[key] = after[key] - before[key]
    }
    return { ...delta, elapsed: (await tick()) - start }
  }
  /** Types a price into a pricing field with the keyboard; Enter commits it, Escape cancels. */
  const typePrice = async (field, text, key = "Enter") => {
    const box = await evaluate(`(() => {
      const input = document.querySelector('${PANEL} input[data-price-input="${field}"]')
      if (!input) return null
      input.scrollIntoView({ block: "center" })
      const r = input.getBoundingClientRect()
      return { x: r.left + r.width / 2, y: r.top + r.height / 2 }
    })()`)
    assert(box, `#106: the ${field} price field is missing`)
    await frame()
    await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", x: box.x, y: box.y, button: "left", buttons: 1, clickCount: 1 })
    await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", x: box.x, y: box.y, button: "left", buttons: 0, clickCount: 1 })
    await evaluate(`document.querySelector('${PANEL} input[data-price-input="${field}"]').select()`)
    await cdp.send("Input.insertText", { text })
    await frame()
    if (key) {
      const code = key === "Enter" ? 13 : 27
      await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key, code: key, windowsVirtualKeyCode: code })
      await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", key, code: key, windowsVirtualKeyCode: code })
      await frame()
    }
  }
  const forecast = () =>
    evaluate(`(() => {
      const forecast = document.querySelector('${PANEL} .pricing-forecast')
      return forecast ? {
        price: Number(forecast.dataset.forecastPrice),
        accepting: Number(forecast.dataset.forecastAccepting),
        sample: Number(forecast.dataset.forecastSample),
        text: forecast.textContent,
      } : null
    })()`)

  // --- Open the authored park through the real file import -------------------------------------
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  const name = "pricing-park"
  const path = `${process.cwd()}/test-results/${name}.zoo-save.json`
  writeFileSync(path, readFileSync(FIXTURE, "utf8"))
  assert(await clickButton("Park menu"), "#106: park menu control missing")
  await waitFor("the park menu", `Boolean(document.querySelector('${MENU}'))`, 30)
  await evaluate(`[...document.querySelectorAll('${MENU} button')].find((b) => b.textContent.startsWith('Load park'))?.click()`)
  await sleep(150)
  const { root } = await cdp.send("DOM.getDocument", { depth: -1 })
  const { nodeId } = await cdp.send("DOM.querySelector", { nodeId: root.nodeId, selector: 'input[type="file"]' })
  assert(nodeId, "#106: the park file input is missing")
  await cdp.send("DOM.setFileInputFiles", { nodeId, files: [path] })
  await waitFor("the import", `[...document.querySelectorAll('.library-slots > li')].some((item) => item.dataset.slot === ${JSON.stringify(name)})`, 60)
  await evaluate(`(() => {
    const item = [...document.querySelectorAll('.library-slots > li')].find((entry) => entry.dataset.slot === ${JSON.stringify(name)})
    ;[...(item?.querySelectorAll('button') ?? [])].find((b) => b.textContent === 'Load')?.click()
  })()`)
  await sleep(200)
  await evaluate(`[...document.querySelectorAll('${MENU} button')].find((b) => b.textContent === 'Discard changes')?.click()`)
  await waitFor("the park", `(document.querySelector('.message')?.textContent ?? '').startsWith('Park loaded')`, 100)
  await waitFor("the renderer", `Boolean(window.__zooParkView?.ready && window.__zooParkView.camera())`, 200)
  assert(await clickButton("Ⅱ"), "#106: pause control missing")
  await openPrices()
  const start = await report()
  assert(start.open && start.admission === 1200, "#106: the park does not open at the default ticket", start)

  // --- 1. The default ticket over one window ----------------------------------------------------
  const fair = await over(WINDOW)
  assert(fair.admitted > 0 && fair.tooDear === 0, "#106: groups did not come in at the default ticket", fair)

  // --- 2. A much dearer ticket over an equal window --------------------------------------------
  await typePrice("admission", DEAR_TICKET)
  await waitFor("the dear ticket", `document.querySelector('${PANEL}')?.dataset.admissionPrice === '${Number(DEAR_TICKET) * 100}'`, 30)
  const dear = await over(WINDOW)
  assert(dear.tooDear > 0, "#106: nobody was turned away by a dear ticket", dear)
  assert(
    dear.admitted * fair.elapsed < fair.admitted * dear.elapsed,
    "#106: a dear ticket did not admit fewer people",
    { fair, dear },
  )
  const recentText = await evaluate(`document.querySelector('${PANEL} .gate-recent')?.textContent ?? ''`)
  assert(/too dear \(would pay \$/.test(recentText), "#106: the report does not say what turned groups away", recentText)
  await screenshot("test-results/pricing-dear-ticket-desktop.png")

  // --- 3. Use the forecast to choose a ticket every recent group would pay ----------------------
  let chosen = null
  for (const candidate of ["30", "25", "20", "15"]) {
    await typePrice("admission", candidate, null)
    await waitFor(`the forecast at $${candidate}`, `document.querySelector('${PANEL} .pricing-forecast')?.dataset.forecastPrice === '${Number(candidate) * 100}'`, 30)
    const at = await forecast()
    assert(at && at.sample > 0 && /Assumes the same visitors/.test(at.text), "#106: the forecast does not state its assumptions", at)
    if (at.accepting === at.sample) {
      chosen = candidate
      break
    }
  }
  assert(chosen !== null, "#106: no candidate ticket was acceptable to every recent group")
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "Enter", code: "Enter", windowsVirtualKeyCode: 13 })
  await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", key: "Enter", code: "Enter", windowsVirtualKeyCode: 13 })
  await waitFor("the corrected ticket", `document.querySelector('${PANEL}')?.dataset.admissionPrice === '${Number(chosen) * 100}'`, 30)
  const corrected = await over(WINDOW)
  assert(corrected.tooDear === 0, "#106: groups were still turned away for price after the correction", corrected)
  assert(
    corrected.admitted * dear.elapsed > dear.admitted * corrected.elapsed,
    "#106: the corrected ticket did not admit more people",
    { dear, corrected },
  )
  await screenshot("test-results/pricing-corrected-ticket-desktop.png")

  // --- 4. The overpriced stand: read the complaints and the usual price, then correct it -------
  const before = await report()
  assert(before.standTooDear > 0, "#106: the overpriced stand drew no price complaints", before)
  const usual = /Usual \$(\d+(?:\.\d\d)?)/.exec(before.standText)?.[1]
  assert(usual, "#106: the report does not show the stand's usual price", before.standText)
  const overpriced = await over(WINDOW)
  await typePrice(`stand-${before.standId}`, usual)
  await waitFor("the corrected stand price", `document.querySelector('${PANEL} .stand-prices li')?.dataset.standPrice === '${Math.round(Number(usual) * 100)}'`, 30)
  const fixed = await over(WINDOW)
  assert(overpriced.standTooDear > 0 && fixed.standTooDear === 0, "#106: the stand's price complaints did not stop", { overpriced, fixed })
  assert(
    fixed.standSold * overpriced.elapsed > overpriced.standSold * fixed.elapsed,
    "#106: the corrected stand did not sell more",
    { overpriced, fixed },
  )
  await screenshot("test-results/pricing-corrected-stand-desktop.png")

  // --- Phone: close and reopen by touch; an unfinished draft is cancelled ----------------------
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 2, mobile: true })
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 })
  await sleep(400)
  await openPrices()
  const tap = async (text) => {
    const box = await evaluate(`(() => {
      const button = [...document.querySelectorAll('${PANEL} button')].find((b) => b.textContent === ${JSON.stringify(text)})
      if (!button || button.disabled) return null
      button.scrollIntoView({ block: "center" })
      const r = button.getBoundingClientRect()
      return { x: r.left + r.width / 2, y: r.top + r.height / 2 }
    })()`)
    assert(box, `#106: no usable ${text} control on the phone`)
    await frame()
    const hit = await evaluate(`document.elementFromPoint(${box.x}, ${box.y})?.textContent ?? null`)
    assert(hit === text, `#106: ${text} is covered on the phone`, hit)
    await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: box.x, y: box.y, id: 0 }] })
    await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] })
    await frame()
  }
  await tap("Close park")
  await waitFor("the closed park", `document.querySelector('${PANEL}')?.dataset.parkOpen === 'false'`, 30)
  await screenshot("test-results/pricing-closed-phone.png")
  await tap("Open park")
  await waitFor("the reopened park", `document.querySelector('${PANEL}')?.dataset.parkOpen === 'true'`, 30)
  await typePrice("admission", "99", "Escape")
  const kept = await report()
  assert(kept.admission === Number(chosen) * 100, "#106: Escape did not cancel the draft price", kept)
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  await evaluate(`document.querySelector('${WORKSPACE} .window-title button')?.click()`)
  await frame()
  console.log(
    `#106 pricing acceptance passed: at $12 ${fair.admitted} people came in over ${fair.elapsed} park minutes; at $${DEAR_TICKET} ${dear.admitted} came in and ${dear.tooDear} were turned away as too dear; the forecast chose $${chosen} and ${corrected.admitted} came in with nobody turned away; the overpriced stand drew ${overpriced.standTooDear} complaints and sold ${overpriced.standSold}, and at its usual $${usual} it sold ${fixed.standSold} with none; the park was closed and reopened by touch on a phone`,
  )
}
