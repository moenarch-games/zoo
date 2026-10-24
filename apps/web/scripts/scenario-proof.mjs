// #91 browser acceptance: New park → pick a scenario → reproducible start.
//
// Called by capture-park-view-proof.mjs against the same production preview and real headless
// Chromium. Every built-in scenario is picked through the real New park picker at desktop and
// phone sizes; the opening camera must frame the scenario's authored focus, a path is built by
// real canvas picking (mouse on desktop, touch on the phone) on the tile the scenario defines,
// and the recorded session must name the scenario. Starting the same scenario twice must give
// the same recorded start identity and park entities, and a saved park lists its scenario.

import { readFileSync } from "node:fs"

import { enterNewPark, frameWholePark, startPickedScenario } from "./park-library-proof.mjs"

const MENU = `[aria-label="Park menu"][role="dialog"]`
const ORDER = ["empty-starter", "partly-prepared", "living-starter", "recovery"]

const scenario = (id) =>
  JSON.parse(readFileSync(new URL(`../../../crates/zoo-core/scenarios/${id}.json`, import.meta.url)))

/** The grass tile just past the end of the scenario's first authored path run. */
function pathTarget(definition) {
  const run = definition.setup.find((step) => step.kind === "path")
  return { x: Math.max(run.from.x, run.to.x) + 1, y: run.to.y }
}

export async function runScenarioAcceptance(context) {
  const { cdp, evaluate, assert, sleep, previewUrl, screenshot, clickButton, frame } = context
  const waitFor = async (description, expression, attempts = 150) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await evaluate(expression)) return
      await sleep(100)
    }
    const state = await evaluate(`({ menu: document.querySelector('${MENU}')?.textContent ?? null, message: document.querySelector('.message')?.textContent ?? null, renderer: document.querySelector('.park-view')?.dataset.renderer ?? null, hook: Boolean(window.__zooParkView), camera: window.__zooParkView?.camera?.() ?? null })`)
    throw new Error(`#91 acceptance timed out waiting for ${description}: ${JSON.stringify(state)}`)
  }
  const camera = () => evaluate(`window.__zooParkView.camera()`)
  const project = (x, y) => evaluate(`window.__zooParkView.projectWorld([${x + 0.5}, 0, ${y + 0.5}])`)
  const exported = async () => JSON.parse(await evaluate(`window.__zooDiagnostics.exportText()`)).session
  const message = () => evaluate(`document.querySelector('.message')?.textContent ?? ''`)
  const canvasCenter = () =>
    evaluate(`(() => {
      const rect = document.querySelector('.park-canvas').getBoundingClientRect()
      return {x: rect.left + rect.width / 2, y: rect.top + rect.height / 2, w: rect.width, h: rect.height}
    })()`)
  const tapOrClick = async (point, phone) => {
    if (phone) {
      await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: point.x, y: point.y, id: 1 }] })
      await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] })
    } else {
      const mouse = (type, extra = {}) =>
        cdp.send("Input.dispatchMouseEvent", { type, x: point.x, y: point.y, button: "none", ...extra })
      await mouse("mouseMoved")
      await mouse("mousePressed", { button: "left", buttons: 1, clickCount: 1 })
      await mouse("mouseReleased", { button: "left", buttons: 0, clickCount: 1 })
    }
    await frame()
  }

  /** Opens the park menu's New park picker and starts `id`, discarding unsaved progress. */
  const startFromMenu = async (id) => {
    assert(await clickButton("Park menu"), "#91: park menu control missing")
    await waitFor("the park menu", `Boolean(document.querySelector('${MENU}'))`, 30)
    assert(
      await evaluate(`(() => {
        const button = [...document.querySelectorAll('${MENU} button')].find((b) => b.textContent === 'New park')
        button?.click()
        return Boolean(button)
      })()`),
      "#91: New park missing from the park menu",
    )
    await sleep(100)
    assert(await startPickedScenario(context, id), `#91: could not pick ${id}`)
    await sleep(150)
    await evaluate(`[...document.querySelectorAll('${MENU} button')].find((b) => b.textContent === 'Discard changes')?.click()`)
  }

  const verifyOpening = async (id, phone) => {
    const definition = scenario(id)
    const label = `${id} (${phone ? "phone" : "desktop"})`
    await waitFor(`${label} renderer`, `Boolean(window.__zooParkView?.ready && window.__zooParkView.camera())`)
    await waitFor(
      `${label} opening focus`,
      `(() => { const t = window.__zooParkView.camera().target
        return Math.abs(t[0] - ${definition.opening_focus.x + 0.5}) < 0.01 && Math.abs(t[2] - ${definition.opening_focus.y + 0.5}) < 0.01 })()`,
      50,
    )
    const opening = await camera()
    assert(opening.zoom > 1.5, `${label}: opening view is not a close objective frame`, opening)
    // The focused tile is under the middle of the view.
    const center = await canvasCenter()
    const focus = await project(definition.opening_focus.x, definition.opening_focus.y)
    assert(
      focus.visible && Math.abs(focus.x - center.x) < center.w * 0.05 && Math.abs(focus.y - center.y) < center.h * 0.05,
      `${label}: opening focus is not centered`,
      { focus, center },
    )
    const objectives = await evaluate(`[...document.querySelectorAll('[aria-label="Scenario objectives"] li')].map((li) => li.dataset.objective)`)
    assert(
      JSON.stringify(objectives) === JSON.stringify((definition.objectives ?? []).map((objective) => objective.id)),
      `${label}: objective checklist does not match the scenario`,
      objectives,
    )
    assert(
      (await evaluate(`Boolean(document.querySelector('[aria-label="Keeper training checklist"]'))`)) ===
        (definition.tutorial === "keeper_training"),
      `${label}: keeper training visibility does not match the scenario`,
    )
    await screenshot(`test-results/scenario-${id}-${phone ? "phone" : "desktop"}.png`)

    // Opening loop: build one path tile by picking it on the whole-park view.
    await frameWholePark(context)
    assert(await evaluate(`window.__zooParkView.parkFootprintInFrame()`), `${label}: whole park not framed`)
    assert(await clickButton("Ⅱ"), `${label}: pause control missing`)
    assert(await clickButton("Path"), `${label}: path tool missing`)
    const target = pathTarget(definition)
    const point = await project(target.x, target.y)
    assert(point.visible, `${label}: path target ${target.x},${target.y} not visible`, point)
    // Checklists may cover part of a phone's view; collapse them as a player would.
    await evaluate(`document.querySelectorAll('details.tutorial-card').forEach((card) => { card.open = false })`)
    await frame()
    assert(
      await evaluate(`document.elementFromPoint(${point.x}, ${point.y})?.classList.contains('park-canvas') ?? false`),
      `${label}: path target is covered by the HUD`,
      await evaluate(`document.elementFromPoint(${point.x}, ${point.y})?.outerHTML.slice(0, 200) ?? null`),
    )
    await tapOrClick(point, phone)
    await waitFor(`${label} path placement`, `document.querySelector('.message')?.textContent === 'Path built'`, 30)
    const session = await exported()
    assert(session.start.scenario === id, `${label}: session does not record the scenario`, session.start)
    const last = session.commands.at(-1)
    assert(
      last.kind === "place_path" && last.args.x === target.x && last.args.y === target.y && last.result.ok,
      `${label}: canvas picking built the wrong tile`,
      { last, target, message: await message() },
    )
    assert(await clickButton("Inspect"), `${label}: inspect tool missing`)
    return session
  }

  // --- Desktop: every scenario from the start screen and the park menu --------------------------
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  await cdp.send("Page.navigate", { url: previewUrl })
  assert(
    (await enterNewPark(context, { scenario: ORDER[0], wholePark: false })) === "started",
    "#91: start screen did not start the first scenario",
  )
  const starts = {}
  starts[ORDER[0]] = await verifyOpening(ORDER[0], false)
  for (const id of ORDER.slice(1)) {
    await startFromMenu(id)
    starts[id] = await verifyOpening(id, false)
  }

  // Reproducible start: the same scenario again records the same start and entities.
  await startFromMenu("living-starter")
  const again = await verifyOpening("living-starter", false)
  assert(
    JSON.stringify(again.start) === JSON.stringify(starts["living-starter"].start) &&
      JSON.stringify(again.entities) === JSON.stringify(starts["living-starter"].entities),
    "#91: restarting a scenario did not reproduce its start",
    { first: starts["living-starter"].start, again: again.start },
  )

  // A saved park lists its scenario in the library.
  assert(await clickButton("Park menu"), "#91: park menu control missing")
  await waitFor("the park menu", `Boolean(document.querySelector('${MENU}'))`, 30)
  await evaluate(`[...document.querySelectorAll('${MENU} button')].find((b) => b.textContent.startsWith('Save'))?.click()`)
  await waitFor("the save", `document.querySelector('.library-message')?.textContent.startsWith('Saved “') ?? false`, 50)
  await evaluate(`[...document.querySelectorAll('${MENU} button')].find((b) => b.textContent.startsWith('Load park'))?.click()`)
  await waitFor(
    "the saved scenario title",
    `[...document.querySelectorAll('.library-slots > li')].some((li) => li.textContent.includes(${JSON.stringify(scenario("living-starter").title)}))`,
    30,
  )
  await evaluate(`[...document.querySelectorAll('${MENU} button')].find((b) => b.textContent === 'Back')?.click()`)
  await sleep(100)
  await evaluate(`[...document.querySelectorAll('${MENU} button')].find((b) => b.textContent === 'Back to park')?.click()`)
  await sleep(100)

  // --- Phone: every scenario opens framed and picks correctly by touch -----------------------
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 2, mobile: true })
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 })
  for (const id of ORDER) {
    await startFromMenu(id)
    await verifyOpening(id, true)
  }
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  console.log(`#91 scenario acceptance passed: ${ORDER.join(", ")} on desktop and phone`)
}
