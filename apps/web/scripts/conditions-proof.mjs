// #118 browser acceptance: the evening before the lived-in starter's first hot day, the
// forecast and both habitats' heat alerts warn the player; adding a shelter from the alert
// prepares the capybaras, and on the hot day their heat comfort is met while the unprepared
// zebras are uncomfortable. Night lighting keeps the park readable on desktop and phone, the
// display settings change only how it looks, and every condition, alert and score comes from
// zoo-core.
//
// Called by capture-park-view-proof.mjs against the same production preview and real headless
// Chromium. The player opens the authored park (crates/zoo-core/tests/fixtures/
// conditions-park.json, written by zoo-core's own test) through the real file import: day 4 at
// 20:00, spring's last evening, with capybaras in habitat #1 and zebras in habitat #2 and no
// shelter in either.

import { readFileSync, writeFileSync } from "node:fs"

const MENU = `[aria-label="Park menu"][role="dialog"]`
const WORKSPACE = `section[aria-label="Management workspace"]`
const FIXTURE = new URL("../../../crates/zoo-core/tests/fixtures/conditions-park.json", import.meta.url)

export async function runConditionsAcceptance(context) {
  const { cdp, evaluate, assert, sleep, screenshot, clickButton, frame, key } = context
  const message = () => evaluate(`document.querySelector('.message')?.textContent ?? ''`)
  const waitFor = async (description, expression, attempts = 150) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await evaluate(expression)) return
      await sleep(100)
    }
    throw new Error(`#118 acceptance timed out waiting for ${description}: ${await message()}`)
  }
  const expectMessage = (text) =>
    waitFor(`the message "${text}"`, `(document.querySelector('.message')?.textContent ?? '').includes(${JSON.stringify(text)})`, 60)
  const settleCamera = async () => {
    let last = ""
    for (let attempt = 0; attempt < 60; attempt += 1) {
      const now = JSON.stringify(await evaluate("window.__zooParkView.camera()"))
      if (now === last) return
      last = now
      await sleep(150)
    }
  }
  const openTab = async (label) => {
    if (!(await evaluate(`Boolean(document.querySelector('${WORKSPACE}'))`))) {
      assert(await clickButton("Management workspace"), "#118: Manage control missing")
      await waitFor("the workspace", `Boolean(document.querySelector('${WORKSPACE}'))`, 30)
    }
    await evaluate(`[...document.querySelectorAll('${WORKSPACE} [role="tab"]')].find((b) => b.textContent.startsWith(${JSON.stringify(label)}))?.click()`)
    await frame()
  }
  const heatAlerts = () =>
    evaluate(`[...document.querySelectorAll('${WORKSPACE} .alert-item[data-code="heat_shade"]')].map((item) => ({
      severity: [...item.classList].find((name) => name !== 'alert-item'),
      text: item.textContent,
    }))`)
  const forecast = () =>
    evaluate(`[...document.querySelectorAll('.forecast li')].map((li) => ({ day: Number(li.dataset.day), weather: li.dataset.weather }))`)
  const heatPart = () =>
    evaluate(`(() => {
      const li = document.querySelector('.welfare-parts li[data-part="heat"]')
      return li ? { met: li.classList.contains('met'), text: li.textContent } : null
    })()`)
  /** Opens habitat `id`'s inspector from the Habitats table and closes the workspace. */
  const inspectHabitat = async (id) => {
    await openTab("Habitats")
    await evaluate(`[...document.querySelectorAll('${WORKSPACE} tbody tr')].find((row) => row.cells[0]?.textContent === '${id}')?.click()`)
    await frame()
    await clickButton("Close management workspace")
    await waitFor(`habitat #${id}'s inspector`, `document.querySelector('.window-title span')?.textContent === 'Habitat #${id}'`)
    await waitFor(`habitat #${id}'s heat part`, `Boolean(document.querySelector('.welfare-parts li[data-part="heat"]'))`, 30)
  }
  const environment = () => evaluate("window.__zooParkView.environment()")
  const runUntil = async (description, expression) => {
    assert(await clickButton("4×"), "#118: speed control missing")
    await waitFor(description, expression, 900)
    assert(await clickButton("Ⅱ"), "#118: pause control missing")
    await frame()
  }

  // --- Open the park through the real import ------------------------------------------------
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  const name = "conditions"
  const path = `${process.cwd()}/test-results/${name}.zoo-save.json`
  writeFileSync(path, readFileSync(FIXTURE, "utf8"))
  assert(await clickButton("Park menu"), "#118: park menu control missing")
  await waitFor("the park menu", `Boolean(document.querySelector('${MENU}'))`, 30)
  await evaluate(`[...document.querySelectorAll('${MENU} button')].find((b) => b.textContent.startsWith('Load park'))?.click()`)
  await sleep(150)
  const { root } = await cdp.send("DOM.getDocument", { depth: -1 })
  const { nodeId } = await cdp.send("DOM.querySelector", { nodeId: root.nodeId, selector: 'input[type="file"]' })
  assert(nodeId, "#118: the park file input is missing")
  await cdp.send("DOM.setFileInputFiles", { nodeId, files: [path] })
  await waitFor(
    "the import",
    `[...document.querySelectorAll('.library-slots > li')].some((item) => item.dataset.slot === ${JSON.stringify(name)})`,
    60,
  )
  await evaluate(`(() => {
    const item = [...document.querySelectorAll('.library-slots > li')].find((entry) => entry.dataset.slot === ${JSON.stringify(name)})
    ;[...(item?.querySelectorAll('button') ?? [])].find((b) => b.textContent === 'Load')?.click()
  })()`)
  await sleep(200)
  await evaluate(`[...document.querySelectorAll('${MENU} button')].find((b) => b.textContent === 'Discard changes')?.click()`)
  await waitFor("the park", `(document.querySelector('.message')?.textContent ?? '').startsWith('Park loaded at day 4, 20:00')`, 100)
  await waitFor("the renderer", `Boolean(window.__zooParkView?.ready && window.__zooParkView.camera())`, 200)
  assert(await clickButton("Ⅱ"), "#118: pause control missing")
  await key("Home")
  await settleCamera()

  // --- Forecast and warnings: tomorrow is hot, both habitats lack shade ----------------------
  const dusk = await environment()
  assert(dusk && dusk.sun.intensity < 2.2, "#118: the evening is lit like midday", dusk)
  assert(await clickButton("Weather and events"), "#118: the weather control is missing")
  await waitFor("the weather report", `Boolean(document.querySelector('.conditions-panel'))`, 30)
  const days = await forecast()
  assert(
    days.length === 3 && days[0].day === 4 && days[1].day === 5 && days[1].weather === "heat",
    "#118: the forecast does not show tomorrow's heat",
    days,
  )
  await screenshot("test-results/conditions-forecast.png")
  await openTab("Alerts")
  const warnings = await heatAlerts()
  assert(
    warnings.length === 2 &&
      warnings.every((alert) => alert.severity === "notice" && alert.text.includes("Tomorrow will be hot")),
    "#118: both habitats are not warned of tomorrow's heat",
    warnings,
  )

  // --- Prepare: add a shelter to habitat #1 straight from its alert -----------------------
  assert(
    await evaluate(`(() => {
      const item = [...document.querySelectorAll('${WORKSPACE} .alert-item[data-code="heat_shade"]')]
        .find((entry) => entry.textContent.includes('habitat #1'))
      const fix = [...(item?.querySelectorAll('button') ?? [])].find((b) => b.textContent.startsWith('Add shelter'))
      fix?.click()
      return Boolean(fix)
    })()`),
    "#118: the heat alert offers no shelter",
  )
  await expectMessage("in habitat #1")
  await waitFor("habitat #1's warning to clear", `document.querySelectorAll('${WORKSPACE} .alert-item[data-code="heat_shade"]').length === 1`, 60)
  await clickButton("Close management workspace")
  await frame()

  // --- Night: the park stays readable, on desktop and phone ---------------------------------
  await runUntil("night", `document.querySelector('.conditions-button')?.dataset.phase === 'night'`)
  const night = await environment()
  assert(
    night.sky.intensity >= 1 && night.sky.intensity < 1.7 && night.sun.intensity > 0,
    "#118: night drops below the readable ambient floor",
    night,
  )
  await screenshot("test-results/conditions-night-desktop.png")
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 2, mobile: true })
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 })
  await sleep(400)
  await frame()
  assert(await evaluate("window.__zooParkView.parkFootprintInFrame()"), "#118: the park leaves the phone frame at night")
  const button = await evaluate(`(() => {
    const box = document.querySelector('.conditions-button')?.getBoundingClientRect()
    return box ? { right: box.right, width: box.width, viewport: window.innerWidth } : null
  })()`)
  assert(button && button.width > 0 && button.right <= button.viewport + 1, "#118: the weather control overflows the phone", button)
  await screenshot("test-results/conditions-night-phone.png")
  // Display settings change the look, never the park.
  assert(await clickButton("Weather and events"), "#118: the weather control is missing on the phone")
  await waitFor("the weather report", `Boolean(document.querySelector('.conditions-panel'))`, 30)
  const before = await evaluate("JSON.parse(window.__zooDiagnostics.exportText()).session.commands.length")
  await evaluate(`[...document.querySelectorAll('.display-settings label')].find((l) => l.textContent.startsWith('Day and night'))?.querySelector('input')?.click()`)
  await frame()
  await waitFor("steady daylight", `window.__zooParkView.environment() === null`, 30)
  const after = await evaluate("JSON.parse(window.__zooDiagnostics.exportText()).session.commands.length")
  assert(before === after, "#118: a display setting issued a game command", { before, after })
  await screenshot("test-results/conditions-display-off-phone.png")
  await evaluate(`[...document.querySelectorAll('.display-settings label')].find((l) => l.textContent.startsWith('Day and night'))?.querySelector('input')?.click()`)
  await frame()
  await clickButton("Close management workspace")
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  await sleep(300)
  await key("Home")
  await settleCamera()

  // --- The hot day: the prepared capybaras are comfortable, the zebras are not -------------
  await runUntil("the hot morning", `document.querySelector('.conditions-button')?.dataset.weather === 'heat' && window.__zooParkView.environment()?.sun?.intensity > 1.5`)
  const hot = await environment()
  assert(hot.sun.color !== "#ffffff", "#118: the hot day is not tinted", hot)
  await inspectHabitat(1)
  const prepared = await heatPart()
  assert(
    prepared.met && prepared.text.includes("100%") && prepared.text.includes("4 of 4 animals"),
    "#118: the prepared habitat is not comfortable in the heat",
    prepared,
  )
  await screenshot("test-results/conditions-heat-prepared.png")
  await inspectHabitat(2)
  const exposed = await heatPart()
  assert(
    !exposed.met && exposed.text.includes("60%") && exposed.text.includes("0 of 3 animals"),
    "#118: the unprepared habitat does not show the heat's cost",
    exposed,
  )
  await screenshot("test-results/conditions-heat-exposed.png")
  await openTab("Alerts")
  const today = await heatAlerts()
  assert(
    today.length === 1 && today[0].severity === "warning" && today[0].text.includes("Today is hot: habitat #2"),
    "#118: the exposed habitat is not warned today",
    today,
  )
  await clickButton("Close management workspace")
  const commands = JSON.parse(await evaluate("window.__zooDiagnostics.exportText()")).session.commands
  assert(
    commands.some((entry) => entry.kind === "add_shelter" && entry.result.ok),
    "#118: the shelter did not go through zoo-core",
  )
  console.log(
    "#118 conditions acceptance passed: tomorrow's heat was forecast and both habitats warned; a shelter added from the alert cleared habitat #1's warning; night kept the readable ambient floor on desktop and phone and the lighting setting issued no command; on the hot day habitat #1's heat comfort was met (100%) while unprepared habitat #2 stayed at 60% with a warning",
  )
}
