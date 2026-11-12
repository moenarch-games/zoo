// #115 browser acceptance: the player identifies a deficit, takes the recovery steps zoo-core
// suggests, makes a budget and loan decision, and recovers at a real cost, with no game over
// and no obligation erased.
//
// Called by capture-park-view-proof.mjs against the same production preview and real headless
// Chromium. The player opens the authored indebted park (crates/zoo-core/tests/fixtures/
// debt-park.json, written by zoo-core's own test) through the real file import: the "Neglected
// corner zoo" borrowed more, built an empty habitat, sells $4 tickets and pays three vendors at a
// broken stand, and has run into the red. The player:
//   1. follows the critical deficit alert to Manage › Finances, which shows the deficit, the
//      days closed in deficit, that borrowing has been covering operating losses, and the
//      recovery steps;
//   2. charges the fair admission price and recalls the vendors from those steps;
//   3. plans a bridge loan in the budget (zoo-core states the cash after, the debt and the
//      interest a day) and borrows it;
//   4. runs the park past midnight: the day closes with interest charged on the debt, the cash
//      is positive again, the park keeps running, and the debt, the interest paid and the
//      deficit days all remain on the books.
// On a phone the panel is used by touch: a small repayment is typed and tapped. Park time and
// every figure come from zoo-core's snapshot.

import { readFileSync, writeFileSync } from "node:fs"

const MENU = `[aria-label="Park menu"][role="dialog"]`
const WORKSPACE = `section[aria-label="Management workspace"]`
const PANEL = `${WORKSPACE} .finance-panel`
const FIXTURE = new URL("../../../crates/zoo-core/tests/fixtures/debt-park.json", import.meta.url)

export async function runFinancesAcceptance(context) {
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
    throw new Error(`#115 acceptance timed out waiting for ${description}: ${JSON.stringify(state)}`)
  }
  const tick = () => evaluate(`JSON.parse(window.__zooDiagnostics.exportText()).session.clock.tick`)
  /** The finances panel's figures, as it shows them. */
  const report = () =>
    evaluate(`(() => {
      const panel = document.querySelector('${PANEL}')
      if (!panel) return null
      const data = panel.dataset
      return {
        cash: Number(data.cash),
        status: data.status,
        deficitDays: Number(data.deficitDays),
        owed: Number(data.loanBalance),
        headroom: Number(data.loanHeadroom),
        interestPaid: Number(data.interestTotal),
        historyDays: Number(data.historyDays),
        verdict: panel.querySelector('.finance-verdict')?.dataset.verdict ?? null,
        steps: [...panel.querySelectorAll('.finance-recovery li')].map((item) => item.dataset.step),
        latestDay: panel.querySelector('.finance-history tbody tr')?.dataset.day ?? null,
        text: panel.querySelector('.finance-status')?.textContent ?? '',
      }
    })()`)
  /** Types an amount into a finances field; `key` Enter or Escape, or none. */
  const typeAmount = async (field, text, key = null) => {
    const box = await evaluate(`(() => {
      const input = document.querySelector('${PANEL} input[data-amount-input="${field}"]')
      if (!input) return null
      input.scrollIntoView({ block: "center" })
      const r = input.getBoundingClientRect()
      return { x: r.left + r.width / 2, y: r.top + r.height / 2 }
    })()`)
    assert(box, `#115: the ${field} field is missing`)
    await frame()
    await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", x: box.x, y: box.y, button: "left", buttons: 1, clickCount: 1 })
    await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", x: box.x, y: box.y, button: "left", buttons: 0, clickCount: 1 })
    await evaluate(`document.querySelector('${PANEL} input[data-amount-input="${field}"]').select()`)
    await cdp.send("Input.insertText", { text })
    await frame()
    if (key) {
      const code = key === "Enter" ? 13 : 27
      await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key, code: key, windowsVirtualKeyCode: code })
      await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", key, code: key, windowsVirtualKeyCode: code })
      await frame()
    }
  }
  const panelButton = (text) =>
    evaluate(`(() => {
      const button = [...document.querySelectorAll('${PANEL} button')].find((b) => b.textContent.startsWith(${JSON.stringify(text)}))
      if (!button || button.disabled) return false
      button.click()
      return true
    })()`)

  // --- Open the authored indebted park through the real file import ---------------------------
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  const name = "debt-park"
  const path = `${process.cwd()}/test-results/${name}.zoo-save.json`
  writeFileSync(path, readFileSync(FIXTURE, "utf8"))
  assert(await clickButton("Park menu"), "#115: park menu control missing")
  await waitFor("the park menu", `Boolean(document.querySelector('${MENU}'))`, 30)
  await evaluate(`[...document.querySelectorAll('${MENU} button')].find((b) => b.textContent.startsWith('Load park'))?.click()`)
  await sleep(150)
  const { root } = await cdp.send("DOM.getDocument", { depth: -1 })
  const { nodeId } = await cdp.send("DOM.querySelector", { nodeId: root.nodeId, selector: 'input[type="file"]' })
  assert(nodeId, "#115: the park file input is missing")
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
  assert(await clickButton("Ⅱ"), "#115: pause control missing")

  // --- 1. The deficit alert leads to the finances report ----------------------------------------
  if (!(await evaluate(`Boolean(document.querySelector('${WORKSPACE}'))`))) {
    assert(await clickButton("Management workspace"), "#115: Manage control missing")
    await waitFor("the workspace", `Boolean(document.querySelector('${WORKSPACE}'))`, 30)
  }
  await evaluate(`[...document.querySelectorAll('${WORKSPACE} [role="tab"]')].find((b) => b.textContent.startsWith('Alerts'))?.click()`)
  await waitFor("the deficit alert", `Boolean(document.querySelector('${WORKSPACE} .alert-item[data-code="cash_deficit"]'))`, 30)
  const alertText = await evaluate(`document.querySelector('${WORKSPACE} .alert-item[data-code="cash_deficit"]').textContent`)
  assert(/in the red/.test(alertText) && /Critical/.test(alertText), "#115: the deficit alert does not explain itself", alertText)
  await evaluate(`[...document.querySelectorAll('${WORKSPACE} .alert-item[data-code="cash_deficit"] button')].find((b) => b.textContent === 'Open finances')?.click()`)
  await waitFor("the finances panel", `Boolean(document.querySelector('${PANEL}'))`, 30)
  const start = await report()
  assert(start.status === "deficit" && start.cash < 0, "#115: the park does not open in deficit", start)
  assert(start.deficitDays >= 1, "#115: no day is recorded as closed in deficit", start)
  assert(start.verdict === "borrowing_covers_losses", "#115: the report does not say borrowing covered the losses", start)
  for (const step of ["raise_admission", "cut_vendors", "borrow"]) {
    assert(start.steps.includes(step), `#115: the ${step} recovery step is missing`, start)
  }
  assert(/wait until cash is positive/.test(start.text), "#115: the panel does not say spending waits", start.text)
  await screenshot("test-results/finances-deficit-desktop.png")

  // --- 2. The suggested recovery steps ----------------------------------------------------------
  assert(await panelButton("Charge $"), "#115: the fair-admission step has no action")
  await waitFor("the fair price", `!document.querySelector('${PANEL} .finance-recovery li[data-step="raise_admission"]')`, 30)
  assert(await panelButton("Recall vendors"), "#115: the vendor step has no action")
  await waitFor("the recalled vendors", `!document.querySelector('${PANEL} .finance-recovery li[data-step="cut_vendors"]')`, 30)

  // --- 3. Plan a bridge loan, then borrow it ----------------------------------------------------
  const bridge = Math.min(start.headroom, -start.cash + 100_000)
  const dollars = (bridge / 100).toFixed(2)
  await typeAmount("borrow", dollars)
  await waitFor("the plan", `document.querySelector('${PANEL} .budget-plan')?.dataset.planBorrow === '${bridge}'`, 30)
  const plan = await evaluate(`(() => {
    const plan = document.querySelector('${PANEL} .budget-plan')
    return { withinLimit: plan.dataset.planWithinLimit, interest: Number(plan.dataset.planInterest), text: plan.textContent }
  })()`)
  assert(plan.withinLimit === "true" && plan.interest > 0 && /interest/.test(plan.text), "#115: the plan does not show the bridge's cost", plan)
  await screenshot("test-results/finances-plan-desktop.png")
  assert(await panelButton("Borrow"), "#115: the borrow control is unavailable")
  await waitFor("the bridge", `Number(document.querySelector('${PANEL}')?.dataset.loanBalance) === ${start.owed + bridge}`, 30)
  const bridged = await report()
  assert(bridged.cash > 0 && bridged.status !== "deficit", "#115: the bridge did not lift cash above zero", bridged)

  // --- 4. Past midnight: interest is charged, obligations remain, the park keeps running --------
  const before = await tick()
  assert(await clickButton("4×"), "#115: speed control missing")
  await waitFor("the day boundary", `Number(document.querySelector('${PANEL}')?.dataset.historyDays) > ${start.historyDays} || document.querySelector('${PANEL} .finance-history tbody tr')?.dataset.day !== '${start.latestDay}'`, 1500)
  await waitFor("five park hours", `JSON.parse(window.__zooDiagnostics.exportText()).session.clock.tick >= ${before + 300}`, 1500)
  assert(await clickButton("Ⅱ"), "#115: pause control missing")
  await frame()
  const after = await report()
  assert(after.interestPaid > start.interestPaid, "#115: no interest was charged on the debt", { start, after })
  assert(after.owed === start.owed + bridge, "#115: the debt changed without a repayment", { start, after, bridge })
  assert(after.deficitDays >= start.deficitDays, "#115: deficit days were erased", { start, after })
  assert(after.cash > 0 && after.status !== "deficit", "#115: the park did not stay out of deficit", after)
  const session = await evaluate(`JSON.parse(window.__zooDiagnostics.exportText()).session.status`)
  assert(session === "running", "#115: the park stopped", session)
  await screenshot("test-results/finances-recovered-desktop.png")

  // --- Phone: a small repayment typed and tapped ------------------------------------------------
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 2, mobile: true })
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 })
  await sleep(400)
  if (!(await evaluate(`Boolean(document.querySelector('${PANEL}'))`))) {
    assert(await clickButton("Management workspace"), "#115: Manage control missing on the phone")
    await waitFor("the workspace", `Boolean(document.querySelector('${WORKSPACE}'))`, 30)
    await evaluate(`[...document.querySelectorAll('${WORKSPACE} [role="tab"]')].find((b) => b.textContent.startsWith('Finances'))?.click()`)
    await waitFor("the finances panel", `Boolean(document.querySelector('${PANEL}'))`, 30)
  }
  await typeAmount("repay", "1")
  const box = await evaluate(`(() => {
    const button = [...document.querySelectorAll('${PANEL} button')].find((b) => b.textContent === 'Repay')
    if (!button || button.disabled) return null
    button.scrollIntoView({ block: "center" })
    const r = button.getBoundingClientRect()
    return { x: r.left + r.width / 2, y: r.top + r.height / 2 }
  })()`)
  assert(box, "#115: no usable Repay control on the phone")
  await frame()
  const hit = await evaluate(`document.elementFromPoint(${box.x}, ${box.y})?.textContent ?? null`)
  assert(hit === "Repay", "#115: Repay is covered on the phone", hit)
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: box.x, y: box.y, id: 0 }] })
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] })
  await waitFor("the repayment", `Number(document.querySelector('${PANEL}')?.dataset.loanBalance) === ${after.owed - 100}`, 30)
  await screenshot("test-results/finances-repaid-phone.png")
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  await evaluate(`document.querySelector('${WORKSPACE} .window-title button')?.click()`)
  await frame()
  console.log(
    `#115 finances acceptance passed: the park opened ${(-start.cash / 100).toFixed(2)} in deficit after ${start.deficitDays} deficit day(s) with borrowing covering losses; the player charged the fair price, recalled the vendors and borrowed a ${dollars} bridge; past midnight it held ${(after.cash / 100).toFixed(2)} in cash, still owed ${(after.owed / 100).toFixed(2)}, had paid ${((after.interestPaid - start.interestPaid) / 100).toFixed(2)} more interest and kept its ${after.deficitDays} deficit day(s) on record; $1 was repaid by touch on a phone`,
  )
}
