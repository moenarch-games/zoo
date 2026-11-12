// #113 browser acceptance: a penguin habitat's climate plant fails, its alert leads the player
// to the habitat, which explains the outage, and replacing the plant restores service; desktop
// and phone.
//
// Called by capture-park-view-proof.mjs against the same production preview and real headless
// Chromium. The player opens the authored park (crates/zoo-core/tests/fixtures/
// utilities-park.json, written by zoo-core's own test) through the real file import: four
// penguins in habitat #1 draw climate control from climate plant #1 and pool water from water
// pump #2; the climate plant is one wear step from failing and no mechanic is hired. Time runs
// until the plant fails; the climate shortage alert appears in the management workspace, its
// Show button opens habitat #1, whose utilities section names the failed plant and offers its
// replacement. The utilities overlay marks the failed plant. Replacing it restores supply and
// clears the alert. Every number, reason and fix comes from zoo-core.

import { readFileSync, writeFileSync } from "node:fs"

const MENU = `[aria-label="Park menu"][role="dialog"]`
const WORKSPACE = `section[aria-label="Management workspace"]`
const UTILITIES = `[data-testid="habitat-utilities"]`
const FIXTURE = new URL("../../../crates/zoo-core/tests/fixtures/utilities-park.json", import.meta.url)

export async function runUtilitiesAcceptance(context) {
  const { cdp, evaluate, assert, sleep, screenshot, clickButton, frame } = context
  const message = () => evaluate(`document.querySelector('.message')?.textContent ?? ''`)
  const alertCodes = () =>
    evaluate(`[...document.querySelectorAll('${WORKSPACE} .alert-item')].map((item) => item.dataset.code)`)
  const waitFor = async (description, expression, attempts = 150) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await evaluate(expression)) return
      await sleep(100)
    }
    const utilities = await evaluate(`document.querySelector('${UTILITIES}')?.textContent ?? null`)
    throw new Error(
      `#113 acceptance timed out waiting for ${description}: ${utilities} | ${JSON.stringify(await alertCodes())} | ${await message()}`,
    )
  }
  const expectMessage = (text) =>
    waitFor(`the message "${text}"`, `(document.querySelector('.message')?.textContent ?? '').includes(${JSON.stringify(text)})`, 60)
  /** Habitat #1's utilities section as the inspector shows it. */
  const utilities = () =>
    evaluate(`(() => {
      const section = document.querySelector('${UTILITIES}')
      if (!section) return null
      const supply = (kind) => {
        const row = section.querySelector('.utility-supply[data-kind="' + kind + '"]')
        return row ? { status: row.dataset.status, text: row.textContent } : null
      }
      return {
        status: section.dataset.status,
        climate: supply('climate'),
        water: supply('water'),
        buttons: [...section.querySelectorAll('button')].map((b) => b.textContent),
      }
    })()`)
  const openAlerts = async () => {
    if (!(await evaluate(`Boolean(document.querySelector('${WORKSPACE}'))`))) {
      assert(await clickButton("Management workspace"), "#113: Manage control missing")
      await waitFor("the workspace", `Boolean(document.querySelector('${WORKSPACE}'))`, 30)
    }
    await evaluate(`[...document.querySelectorAll('${WORKSPACE} [role="tab"]')].find((b) => b.textContent.startsWith('Alerts'))?.click()`)
    await frame()
  }
  const hasAlert = (code) =>
    `Boolean(document.querySelector('${WORKSPACE} .alert-item[data-code=${JSON.stringify(code)}]'))`

  // --- Open the park through the real import ------------------------------------------------
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  const name = "utilities"
  const path = `${process.cwd()}/test-results/${name}.zoo-save.json`
  writeFileSync(path, readFileSync(FIXTURE, "utf8"))
  assert(await clickButton("Park menu"), "#113: park menu control missing")
  await waitFor("the park menu", `Boolean(document.querySelector('${MENU}'))`, 30)
  await evaluate(`[...document.querySelectorAll('${MENU} button')].find((b) => b.textContent.startsWith('Load park'))?.click()`)
  await sleep(150)
  const { root } = await cdp.send("DOM.getDocument", { depth: -1 })
  const { nodeId } = await cdp.send("DOM.querySelector", { nodeId: root.nodeId, selector: 'input[type="file"]' })
  assert(nodeId, "#113: the park file input is missing")
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
  await waitFor("the park", `(document.querySelector('.message')?.textContent ?? '').startsWith('Park loaded')`, 100)
  await waitFor("the renderer", `Boolean(window.__zooParkView?.ready && window.__zooParkView.camera())`, 200)
  assert(await clickButton("Ⅱ"), "#113: pause control missing")

  // --- Outage: run until the climate plant fails and its shortage alert appears ------------
  await openAlerts()
  assert(!(await evaluate(hasAlert("climate_shortage"))), "#113: the park starts short of climate control", await alertCodes())
  assert(await clickButton("4×"), "#113: speed control missing")
  await waitFor("the climate shortage alert", hasAlert("climate_shortage"), 600)
  assert(await clickButton("Ⅱ"), "#113: pause control missing")
  await frame()
  const alertText = await evaluate(
    `document.querySelector('${WORKSPACE} .alert-item[data-code="climate_shortage"]')?.textContent ?? ''`,
  )
  assert(
    alertText.includes("Habitat #1 gets 0 of 4 climate control units because a connected plant has failed"),
    "#113: the shortage alert does not explain the outage",
    alertText,
  )
  assert((await alertCodes()).includes("facility_failed"), "#113: the failed plant raises no alert", await alertCodes())
  await screenshot("test-results/utilities-outage-alert.png")

  // --- Follow the alert to the habitat, which explains the outage --------------------------
  assert(
    await evaluate(`(() => {
      const item = document.querySelector('${WORKSPACE} .alert-item[data-code="climate_shortage"]')
      const show = [...(item?.querySelectorAll('button') ?? [])].find((b) => b.textContent === 'Show')
      show?.click()
      return Boolean(show)
    })()`),
    "#113: the shortage alert has no Show button",
  )
  await frame()
  await waitFor("habitat #1's utilities", `Boolean(document.querySelector('${UTILITIES}'))`, 60)
  const outage = await utilities()
  assert(
    outage.climate?.status === "short" &&
      outage.climate.text.includes("0/4") &&
      outage.climate.text.includes("a connected plant has failed") &&
      outage.water?.status === "supplied",
    "#113: the habitat does not explain the climate outage",
    outage,
  )
  assert(
    outage.buttons.some((text) => text.startsWith("Replace plant #1")),
    "#113: the habitat offers no replacement for the failed plant",
    outage,
  )
  await screenshot("test-results/utilities-outage-habitat.png")

  // --- The utilities overlay marks the failed plant ------------------------------------------
  await evaluate(`(() => {
    const select = document.querySelector('select[aria-label="Overlay"]')
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(select, 'utilities')
    select.dispatchEvent(new Event('change', { bubbles: true }))
  })()`)
  await frame()
  await waitFor(
    "the utilities overlay",
    `Number([...document.querySelectorAll('.overlay-legend .legend-swatch.bad')][0]?.parentElement?.querySelector('strong')?.textContent ?? 0) >= 1`,
    60,
  )
  await screenshot("test-results/utilities-overlay.png")

  // --- Phone: the outage explanation and its fix fit a touch viewport ------------------------
  await clickButton("Close management workspace")
  await frame()
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 2, mobile: true })
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 })
  await sleep(400)
  const fits = await evaluate(`(() => {
    const section = document.querySelector('${UTILITIES}')
    if (!section) return { present: false }
    section.scrollIntoView({ block: 'nearest' })
    const box = section.getBoundingClientRect()
    return { present: true, width: box.width, right: box.right, viewport: window.innerWidth }
  })()`)
  assert(
    fits.present && fits.width > 0 && fits.right <= fits.viewport + 1,
    "#113: the utilities section is missing or overflows the phone viewport",
    fits,
  )
  await screenshot("test-results/utilities-phone.png")
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  await sleep(300)

  // --- Restore: replace the plant from the habitat ------------------------------------------
  await evaluate(`[...document.querySelectorAll('${UTILITIES} button')].find((b) => b.textContent.startsWith('Replace plant #1'))?.click()`)
  await expectMessage("Climate plant #1 replaced for $160")
  await waitFor("restored supply", `document.querySelector('${UTILITIES}')?.dataset.status === 'supplied'`, 60)
  const restored = await utilities()
  assert(restored.climate.text.includes("4/4"), "#113: climate control is not restored", restored)
  await openAlerts()
  await waitFor("the shortage alert to clear", `!${hasAlert("climate_shortage")} && !${hasAlert("facility_failed")}`, 60)
  const commands = JSON.parse(await evaluate("window.__zooDiagnostics.exportText()")).session.commands
  assert(
    commands.some((entry) => entry.kind === "replace_utility" && entry.result.ok),
    "#113: the replacement did not go through zoo-core",
  )
  await screenshot("test-results/utilities-restored.png")
  await evaluate(`(() => {
    const select = document.querySelector('select[aria-label="Overlay"]')
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(select, '')
    select.dispatchEvent(new Event('change', { bubbles: true }))
  })()`)
  console.log(
    "#113 utilities acceptance passed: climate plant #1 failed, the climate shortage alert led to habitat #1, which named the failed plant and its fix, the utilities overlay marked it, and replacing it restored 4/4 climate control and cleared the alerts; the section fits a phone",
  )
}
