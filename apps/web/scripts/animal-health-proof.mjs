// #109 browser acceptance: one capybara is followed from a health warning through illness,
// quarantine and a vet's treatment to recovery and its homecoming; desktop and phone.
//
// Called by capture-park-view-proof.mjs against the same production preview and real headless
// Chromium. The player opens the authored park (crates/zoo-core/tests/fixtures/
// animal-health-park.json, written by zoo-core's own test) through the real file import:
// capybaras #1 and #2 live in habitat #1, whose only keeper does not clean, and are close to
// falling ill with the sniffles; habitat #2 is set aside for quarantine; no vet is hired and
// the depot has no medicine. The player picks capybara #1 in the 3D park and reads zoo-core's
// warning in its inspector. Time runs until it is unwell and its treatment is blocked for want
// of a vet; the player moves it into quarantine, then hires a vet and buys medicine with the
// inspector's own fix buttons. The vet walks a kit to the quarantine habitat and treats it;
// it recovers there and walks home by itself. Every status, reason and move comes from
// zoo-core.

import { readFileSync, writeFileSync } from "node:fs"

const MENU = `[aria-label="Park menu"][role="dialog"]`
const HEALTH = `.animal-card section.animal-health`
const FIXTURE = new URL("../../../crates/zoo-core/tests/fixtures/animal-health-park.json", import.meta.url)

export async function runAnimalHealthAcceptance(context) {
  const { cdp, evaluate, assert, sleep, screenshot, clickButton, frame, mouse } = context
  const message = () => evaluate(`document.querySelector('.message')?.textContent ?? ''`)
  const waitFor = async (description, expression, attempts = 150) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await evaluate(expression)) return
      await sleep(100)
    }
    const health = await evaluate(`document.querySelector('${HEALTH}')?.textContent ?? null`)
    throw new Error(`#109 acceptance timed out waiting for ${description}: ${health} | ${await message()}`)
  }
  const expectMessage = (text) =>
    waitFor(`the message "${text}"`, `(document.querySelector('.message')?.textContent ?? '').includes(${JSON.stringify(text)})`, 60)
  /** The inspected animal's health as the inspector shows it. */
  const health = () =>
    evaluate(`(() => {
      const card = document.querySelector('.animal-card')
      const section = card?.querySelector('section.animal-health')
      if (!section) return null
      return {
        id: Number(card.dataset.animalId),
        status: section.dataset.status,
        summary: section.querySelector('[data-testid="animal-health"]')?.textContent ?? '',
        quarantine: section.querySelector('[data-testid="quarantine-status"]')?.textContent ?? null,
        buttons: [...section.querySelectorAll('button')].map((b) => ({ text: b.textContent, disabled: b.disabled })),
        habitat: card.querySelector('dl')?.textContent ?? '',
      }
    })()`)
  const status = (value) => `document.querySelector('${HEALTH}')?.dataset.status === ${JSON.stringify(value)}`
  const pressHealth = async (label) => {
    const pressed = await evaluate(`(() => {
      const button = [...document.querySelectorAll('${HEALTH} button')].find((b) => b.textContent.startsWith(${JSON.stringify(label)}))
      if (!button || button.disabled) return false
      button.click()
      return true
    })()`)
    assert(pressed, `#109: the inspector's ${label} control is missing or disabled`, await health())
    await frame()
  }
  /** Runs the park at 4× until `expression` holds, then pauses. */
  const runUntil = async (description, expression, attempts = 900) => {
    assert(await clickButton("4×"), "#109: speed control missing")
    await waitFor(description, expression, attempts)
    assert(await clickButton("Ⅱ"), "#109: pause control missing")
    await frame()
  }

  // --- Open the neglected park through the real import --------------------------------------
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  const name = "animal-health"
  const path = `${process.cwd()}/test-results/${name}.zoo-save.json`
  writeFileSync(path, readFileSync(FIXTURE, "utf8"))
  assert(await clickButton("Park menu"), "#109: park menu control missing")
  await waitFor("the park menu", `Boolean(document.querySelector('${MENU}'))`, 30)
  await evaluate(`[...document.querySelectorAll('${MENU} button')].find((b) => b.textContent.startsWith('Load park'))?.click()`)
  await sleep(150)
  const { root } = await cdp.send("DOM.getDocument", { depth: -1 })
  const { nodeId } = await cdp.send("DOM.querySelector", { nodeId: root.nodeId, selector: 'input[type="file"]' })
  assert(nodeId, "#109: the park file input is missing")
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
  assert(await clickButton("Ⅱ"), "#109: pause control missing")

  // --- Warning: pick capybara #1 in the park and read its health ----------------------------
  const point = await evaluate(`window.__zooParkView.animalPoint(1)`)
  assert(point?.visible, "#109: capybara #1 is not on screen", point)
  await mouse("mouseMoved", point)
  await mouse("mousePressed", point, { button: "left", buttons: 1, clickCount: 1 })
  await mouse("mouseReleased", point, { button: "left", buttons: 0, clickCount: 1 })
  await frame()
  // Picking watches first (#141); the inspector is the watch label's deliberate Details step.
  await waitFor("capybara #1's watch label", `Boolean(document.querySelector('[data-animal-watch="1"] [data-action="animal-details"]'))`)
  await evaluate(`document.querySelector('[data-action="animal-details"]').click()`)
  await waitFor("capybara #1's inspector", `document.querySelector('.animal-card')?.dataset.animalId === '1'`)
  const warning = await health()
  assert(
    warning?.status === "at_risk" && warning.summary.includes("At risk of the sniffles: habitat #1 is dirty"),
    "#109: the inspector does not warn about the dirty habitat",
    warning,
  )
  await screenshot("test-results/animal-health-warning.png")

  // --- Illness: unwell, resting, and blocked for want of a vet ------------------------------
  await runUntil("capybara #1 to fall ill", status("unwell"))
  const unwell = await health()
  assert(
    unwell.summary.includes("Unwell with the sniffles: sneezing and resting") &&
      unwell.summary.includes("No vet is on staff"),
    "#109: the illness or its blocked treatment is not explained",
    unwell,
  )
  assert(
    unwell.buttons.some((b) => b.text.startsWith("Hire vet") && !b.disabled),
    "#109: the inspector offers no vet to hire",
    unwell,
  )
  await screenshot("test-results/animal-health-unwell.png")

  // --- Quarantine: the patient moves apart from its group -----------------------------------
  await pressHealth("Move to quarantine")
  await expectMessage("(#1) moved from habitat #1 into quarantine habitat #2")
  await waitFor("the quarantine status", `Boolean(document.querySelector('${HEALTH} [data-testid="quarantine-status"]'))`, 30)
  const quarantined = await health()
  assert(
    quarantined.quarantine.includes("returns to habitat #1 once recovered") && quarantined.habitat.includes("habitat #2"),
    "#109: the patient is not shown in quarantine habitat #2",
    quarantined,
  )

  // --- Treatment: hire a vet and buy medicine from the inspector's own fixes ----------------
  await pressHealth("Hire vet")
  await expectMessage("hired at the animal-care depot")
  await waitFor("the medicine fix", `[...document.querySelectorAll('${HEALTH} button')].some((b) => b.textContent.startsWith('Buy'))`, 30)
  await pressHealth("Buy")
  await expectMessage("medicine kits for the animal-care depot")
  await runUntil(
    "the vet to come",
    `/Vet #\\d+ is (on the way|treating it)/.test(document.querySelector('${HEALTH} [data-testid="animal-health"]')?.textContent ?? '')`,
  )
  await screenshot("test-results/animal-health-vet.png")
  await runUntil("the treatment", status("recovering"))
  const recovering = await health()
  assert(
    recovering.summary.startsWith("Treated for the sniffles; recovering"),
    "#109: recovery is not shown after the treatment",
    recovering,
  )
  await screenshot("test-results/animal-health-recovering.png")

  // --- Recovery and homecoming --------------------------------------------------------------
  await runUntil("recovery and homecoming", `${status("healthy")} && (document.querySelector('.animal-card dl')?.textContent ?? '').includes('habitat #1')`, 1800)
  const home = await health()
  assert(home.quarantine === null, "#109: the recovered capybara is still in quarantine", home)
  const commands = JSON.parse(await evaluate("window.__zooDiagnostics.exportText()")).session.commands
  for (const kind of ["quarantine_animal", "hire_vet", "buy_medicine"]) {
    assert(
      commands.some((entry) => entry.kind === kind && entry.result.ok),
      `#109: ${kind} did not go through zoo-core`,
    )
  }
  await screenshot("test-results/animal-health-recovered.png")

  // --- Phone: the health section fits a touch viewport --------------------------------------
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 2, mobile: true })
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 })
  await sleep(400)
  const fits = await evaluate(`(() => {
    const section = document.querySelector('${HEALTH}')
    if (!section) return false
    const box = section.getBoundingClientRect()
    return box.width > 0 && box.right <= window.innerWidth + 1
  })()`)
  assert(fits, "#109: the health section overflows the phone viewport")
  await screenshot("test-results/animal-health-phone.png")
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  console.log(
    "#109 animal-health acceptance passed: capybara #1 was warned about its dirty habitat, fell ill with the sniffles, waited blocked for a vet, moved into quarantine habitat #2, was treated by a newly hired vet with depot medicine, recovered there and walked home to habitat #1; the health section fits a phone",
  )
}
