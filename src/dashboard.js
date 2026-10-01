const { invoke } = window.__TAURI__.core

const countdownEl = document.getElementById('countdown')
const listEl = document.getElementById('list')

document.getElementById('recordNow').addEventListener('click', () => invoke('record_now'))
document.getElementById('openSettings').addEventListener('click', () => invoke('open_settings'))

function renderCountdown({ activeAccumulatedMs, activeHourMs, paused }) {
  if (paused) {
    countdownEl.textContent = '⏸ paused (sleep/locked)'
    countdownEl.classList.add('paused')
    return
  }
  const remainingMin = Math.max(0, Math.ceil((activeHourMs - activeAccumulatedMs) / 60000))
  countdownEl.textContent = `~${remainingMin} min of activity`
  countdownEl.classList.remove('paused')
}

function renderEntries(entries) {
  listEl.replaceChildren()

  if (!entries.length) {
    const empty = document.createElement('div')
    empty.className = 'empty'
    empty.textContent = 'No entries in the last 30 days'
    listEl.appendChild(empty)
    return
  }

  let currentDay = null
  for (const entry of entries) {
    if (entry.date !== currentDay) {
      currentDay = entry.date
      const dayLabel = document.createElement('div')
      dayLabel.className = 'day-label'
      dayLabel.textContent = currentDay
      listEl.appendChild(dayLabel)
    }

    const row = document.createElement('div')
    row.className = 'entry'
    const time = document.createElement('div')
    time.className = 'entry-time'
    time.textContent = entry.time
    const text = document.createElement('div')
    text.className = 'entry-text'
    text.textContent = entry.text
    row.append(time, text)
    listEl.appendChild(row)
  }
}

// The entries are read from disk once, when the flyout opens. The countdown is
// a cheap in-memory value, so it is refreshed every second.
invoke('get_dashboard_data').then((data) => {
  renderEntries(data.entries)
  renderCountdown(data)
})
setInterval(() => invoke('get_tick_state').then(renderCountdown), 1000)
