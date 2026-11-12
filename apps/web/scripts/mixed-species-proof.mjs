// #108 browser acceptance: a mixed-species exhibit is previewed, established, diagnosed and
// fixed through the real habitat inspector; desktop and phone.
//
// Called by capture-park-view-proof.mjs against the same production preview and real headless
// Chromium. The player opens the authored park (crates/zoo-core/tests/fixtures/
// mixed-species-park.json, written by zoo-core's own test) through the real file import: three
// zebras and one basic shelter in a staffed steel-mesh yard. With a group size of two, the
// adoption list shows zoo-core's verdicts: giraffes mix with the zebras (with the authored
// reason, the shared space, feed use and upkeep, and a shared-shelter warning), elephants are
// refused with their reason, flamingos have no profile. Two giraffes are adopted at once; the
// residents list both species and the welfare breakdown diagnoses the shared shelter as short.
// Placing a large shelter with the design brush resolves it. Every verdict, number and message
// comes from zoo-core.

import { readFileSync, writeFileSync } from "node:fs"

const MENU = `[aria-label="Park menu"][role="dialog"]`
const FIXTURE = new URL("../../../crates/zoo-core/tests/fixtures/mixed-species-park.json", import.meta.url)

export async function runMixedSpeciesAcceptance(context) {
  const { cdp, evaluate, assert, sleep, screenshot, clickButton, frame, tile, mouse, key } = context
  const message = () => evaluate(`document.querySelector('.message')?.textContent ?? ''`)
  const waitFor = async (description, expression, attempts = 150) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await evaluate(expression)) return
      await sleep(100)
    }
    throw new Error(`#108 acceptance timed out waiting for ${description}: ${await message()}`)
  }
  const expectMessage = (text) =>
    waitFor(`the message "${text}"`, `(document.querySelector('.message')?.textContent ?? '').includes(${JSON.stringify(text)})`, 60)
  const tap = async (x, y) => {
    const point = await tile(x, y)
    await mouse("mouseMoved", point)
    await mouse("mousePressed", point, { button: "left", buttons: 1, clickCount: 1 })
    await mouse("mouseReleased", point, { button: "left", buttons: 0, clickCount: 1 })
    await frame()
    await sleep(150)
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
  /** One species' adoption choice: its button state and zoo-core's preview lines. */
  const choice = (species) =>
    evaluate(`(() => {
      const row = document.querySelector('.species-choice[data-species="${species}"]')
      if (!row) return null
      return {
        disabled: row.querySelector('button').disabled,
        lines: [...row.querySelectorAll('.arrival-preview small')].map((line) => line.textContent),
      }
    })()`)
  const shelterPart = () =>
    evaluate(`(() => {
      const li = document.querySelector('.welfare-parts li[data-part="shelter"]')
      return li ? { met: li.classList.contains('met'), text: li.textContent } : null
    })()`)
  const residents = () =>
    evaluate(`[...document.querySelectorAll('ul.resident-groups li')].map((li) => ({ species: li.dataset.species, text: li.textContent }))`)

  // --- Open the zebra yard through the real import ------------------------------------------
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  const name = "mixed-species"
  const path = `${process.cwd()}/test-results/${name}.zoo-save.json`
  writeFileSync(path, readFileSync(FIXTURE, "utf8"))
  assert(await clickButton("Park menu"), "#108: park menu control missing")
  await waitFor("the park menu", `Boolean(document.querySelector('${MENU}'))`, 30)
  await evaluate(`[...document.querySelectorAll('${MENU} button')].find((b) => b.textContent.startsWith('Load park'))?.click()`)
  await sleep(150)
  const { root } = await cdp.send("DOM.getDocument", { depth: -1 })
  const { nodeId } = await cdp.send("DOM.querySelector", { nodeId: root.nodeId, selector: 'input[type="file"]' })
  assert(nodeId, "#108: the park file input is missing")
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
  assert(await clickButton("Ⅱ"), "#108: pause control missing")
  await key("Home")
  await settleCamera()

  assert(await clickButton("Inspect"), "#108: inspect tool missing")
  await tap(5, 12)
  await waitFor("habitat #1's inspector", `document.querySelector('.window-title span')?.textContent === 'Habitat #1'`)
  await waitFor("the adoption list", `Boolean(document.querySelector('.species-choice[data-species="giraffe"]'))`)

  // --- Preview: two giraffes mix with the zebras; elephants and flamingos do not -------------
  assert(await clickButton("More animals"), "#108: group size control missing")
  await waitFor("a group of two", `document.querySelector('.adopt-count input')?.value === '2'`, 30)
  await frame()
  const giraffe = await choice("giraffe")
  assert(giraffe && !giraffe.disabled, "#108: giraffes cannot be previewed into the zebra yard", giraffe)
  assert(giraffe.lines.some((line) => line.startsWith("Mixes with Zebra") && line.includes("browse")),
    "#108: the giraffe preview does not explain the pairing", giraffe)
  assert(giraffe.lines.some((line) => line.includes("feed use 12→20/h")),
    "#108: the giraffe preview does not show the shared feed use", giraffe)
  assert(giraffe.lines.some((line) => line.includes("Shared shelter: places for 4 of 5 animals")),
    "#108: the giraffe preview does not warn about the shared shelter", giraffe)
  const elephant = await choice("elephant")
  assert(elephant?.disabled && elephant.lines.some((line) => line.includes("trample")),
    "#108: elephants are not refused with their reason", elephant)
  const flamingo = await choice("flamingo")
  assert(flamingo?.disabled && flamingo.lines.some((line) => line.includes("No mixed-exhibit profile")),
    "#108: an unprofiled pair is not refused", flamingo)
  await screenshot("test-results/mixed-species-preview.png")

  // --- Establish: both giraffes arrive in one adoption ----------------------------------------
  assert(await clickButton("Giraffe"), "#108: giraffe adoption missing")
  await expectMessage("2 Giraffe adopted into habitat #1")
  await waitFor("both resident species", `document.querySelectorAll('ul.resident-groups li').length === 2`, 30)
  const groups = await residents()
  assert(
    groups[0].species === "zebra" && groups[0].text.includes("3 of 3+") &&
      groups[1].species === "giraffe" && groups[1].text.includes("2 of 2+"),
    "#108: the residents do not list each species' own group",
    groups,
  )
  assert(
    (await evaluate(`document.querySelector('.habitat-name')?.textContent ?? ''`)) === "Zebra & Giraffe",
    "#108: the inspector does not name the mixed exhibit",
  )

  // --- Diagnose: the shared shelter is short ---------------------------------------------------
  const short = await shelterPart()
  assert(short && !short.met && short.text.includes("4 of 5 animals"),
    "#108: the welfare breakdown does not diagnose the shared shelter", short)
  await screenshot("test-results/mixed-species-diagnosed.png")

  // --- Resolve: a large shelter covers every resident ------------------------------------------
  assert(await clickButton("Design terrain"), "#108: design terrain control missing")
  await waitFor("the brushes", `Boolean(document.querySelector('[data-brush="object:large_shelter"]'))`, 30)
  await evaluate(`document.querySelector('[data-brush="object:large_shelter"]').click()`)
  await sleep(150)
  await tap(3, 12)
  await expectMessage("Place Large shelter at 3,12")
  await waitFor("the shelter to be met", `document.querySelector('.welfare-parts li[data-part="shelter"]')?.classList.contains('met') ?? false`, 30)
  const fixed = await shelterPart()
  assert(fixed.text.includes("5 of 5 animals"), "#108: the shelter does not cover all five", fixed)
  const commands = JSON.parse(await evaluate("window.__zooDiagnostics.exportText()")).session.commands
  assert(
    commands.some((entry) => entry.kind === "adopt_group" && entry.args.count === 2 && entry.result.ok),
    "#108: the group adoption did not go through zoo-core",
  )
  await screenshot("test-results/mixed-species-resolved.png")
  assert(await clickButton("Stop designing"), "#108: stop designing control missing")
  await waitFor("the inspector", `Boolean(document.querySelector('ul.resident-groups'))`, 30)

  // --- Phone: the adoption verdicts fit a touch viewport -----------------------------------
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 2, mobile: true })
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 })
  await sleep(400)
  const fits = await evaluate(`(() => {
    const rows = [...document.querySelectorAll('.species-choice, ul.resident-groups')]
    return rows.length > 0 && rows.every((row) => {
      const box = row.getBoundingClientRect()
      return box.width > 0 && box.right <= window.innerWidth + 1
    })
  })()`)
  assert(fits, "#108: the residents or adoption choices overflow the phone viewport")
  await screenshot("test-results/mixed-species-phone.png")
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  console.log(
    "#108 mixed-species acceptance passed: two giraffes previewed into a zebra yard with the authored pairing, shared feed use and a shared-shelter warning; elephants and flamingos refused with reasons; both giraffes adopted in one zoo-core command; the shared shelter diagnosed short (4 of 5) and resolved by a large shelter; the choices fit a phone",
  )
}
