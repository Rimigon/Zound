// SVG icon sprite. Все символы через currentColor, размеры через .icon /
// .icon-lg / .icon-xl. Использование: ic("i-play") → строка с <svg><use/>.

const SPRITE = `
<svg xmlns="http://www.w3.org/2000/svg"><defs>

<symbol id="i-play" viewBox="0 0 16 16">
  <path d="M4.5 3.5L12 8L4.5 12.5V3.5Z" fill="currentColor" stroke="currentColor" stroke-linejoin="round"/>
</symbol>

<symbol id="i-stop" viewBox="0 0 16 16">
  <rect x="4" y="4" width="8" height="8" rx="1.5" fill="currentColor"/>
</symbol>

<symbol id="i-mute" viewBox="0 0 20 20">
  <path d="M3 8v4h3l4 3V5L6 8H3Z"/>
  <path d="M13 7l4 6M17 7l-4 6"/>
</symbol>

<symbol id="i-unmute" viewBox="0 0 20 20">
  <path d="M3 8v4h3l4 3V5L6 8H3Z"/>
  <path d="M13 7c1.5 1 1.5 5 0 6"/>
  <path d="M15.5 5c3 2 3 8 0 10"/>
</symbol>

<symbol id="i-sun" viewBox="0 0 20 20">
  <circle cx="10" cy="10" r="3.2"/>
  <path d="M10 3v1.5M10 15.5V17M3 10h1.5M15.5 10H17M5 5l1 1M14 14l1 1M5 15l1-1M14 6l1-1"/>
</symbol>

<symbol id="i-moon" viewBox="0 0 20 20">
  <path d="M16 11.5A6 6 0 1 1 8.5 4a5 5 0 0 0 7.5 7.5Z"/>
</symbol>

<symbol id="i-palette" viewBox="0 0 20 20">
  <path d="M10 3a7 7 0 0 0 0 14c1 0 1.5-.5 1.5-1.2 0-.5-.3-.8-.3-1.3 0-.8.6-1.5 1.5-1.5h1.5a3.3 3.3 0 0 0 3.3-3.3C17.5 6.4 14.1 3 10 3Z"/>
  <circle cx="6" cy="9" r="1" fill="currentColor" stroke="none"/>
  <circle cx="8.5" cy="6" r="1" fill="currentColor" stroke="none"/>
  <circle cx="12" cy="6" r="1" fill="currentColor" stroke="none"/>
  <circle cx="14.5" cy="9" r="1" fill="currentColor" stroke="none"/>
</symbol>

<symbol id="i-test" viewBox="0 0 16 16">
  <path d="M3 8h2l1.5-3 3 6 1.5-3H13"/>
</symbol>

<symbol id="i-sine" viewBox="0 0 16 16">
  <path d="M2 8c1.5-3 3-3 4 0s2.5 3 4 0 2.5-3 4 0"/>
</symbol>

<symbol id="i-click" viewBox="0 0 16 16">
  <circle cx="8" cy="8" r="1.4" fill="currentColor"/>
  <path d="M4 8l1.5-1M11.5 7L13 8M5.5 11.5l.8-1.2M9.7 10.3l.8 1.2M5.5 4.5l.8 1.2M9.7 5.7l.8-1.2"/>
</symbol>

<symbol id="i-metronome" viewBox="0 0 16 16">
  <path d="M5 13h6L9.5 4h-3L5 13Z"/>
  <path d="M5 11h6"/>
  <path d="M9 11l-2.5-5"/>
</symbol>

<symbol id="i-restart" viewBox="0 0 16 16">
  <path d="M13 8a5 5 0 1 1-1.5-3.5L13 6"/>
  <path d="M13 3v3h-3"/>
</symbol>

<symbol id="i-eq" viewBox="0 0 16 16">
  <path d="M4 3v10M8 3v10M12 3v10"/>
  <circle cx="4" cy="6" r="1.4" fill="currentColor"/>
  <circle cx="8" cy="10" r="1.4" fill="currentColor"/>
  <circle cx="12" cy="5" r="1.4" fill="currentColor"/>
</symbol>

<symbol id="i-group" viewBox="0 0 16 16">
  <circle cx="6" cy="6" r="2.5"/>
  <circle cx="11" cy="9" r="2"/>
  <path d="M2 13c.5-2 2-3 4-3s3.5 1 4 3"/>
</symbol>

<symbol id="i-rename" viewBox="0 0 16 16">
  <path d="M3 11.5L11 3.5l1.5 1.5L4.5 13l-2 .5L3 11.5Z"/>
</symbol>

<symbol id="i-plus" viewBox="0 0 16 16">
  <path d="M8 3v10M3 8h10"/>
</symbol>

<symbol id="i-minus" viewBox="0 0 16 16">
  <path d="M3 8h10"/>
</symbol>

<symbol id="i-alert" viewBox="0 0 16 16">
  <path d="M8 2.5L14 13H2L8 2.5Z"/>
  <path d="M8 7v3" stroke-linecap="round"/>
  <circle cx="8" cy="11.5" r="0.7" fill="currentColor" stroke="none"/>
</symbol>

<symbol id="i-check" viewBox="0 0 16 16">
  <path d="M3.5 8.5L7 12l5.5-7.5"/>
</symbol>

<symbol id="i-drift" viewBox="0 0 16 16">
  <path d="M2 8h3l1.5-3 1.5 6 1.5-4 1.5 4 1-3h2"/>
</symbol>

<symbol id="i-speaker" viewBox="0 0 16 16">
  <rect x="4" y="2.5" width="8" height="11" rx="1.5"/>
  <circle cx="8" cy="9.5" r="2"/>
  <circle cx="8" cy="5" r="0.6" fill="currentColor" stroke="none"/>
</symbol>

<symbol id="i-headphones" viewBox="0 0 16 16">
  <path d="M3 10v-2a5 5 0 0 1 10 0v2"/>
  <rect x="2.5" y="9.5" width="2.5" height="4" rx="1"/>
  <rect x="11" y="9.5" width="2.5" height="4" rx="1"/>
</symbol>

<symbol id="i-bluetooth" viewBox="0 0 16 16">
  <path d="M6 4l5 4-5 4 3-3-3-3V4l5 4"/>
</symbol>

<symbol id="i-mic" viewBox="0 0 16 16">
  <rect x="6.5" y="2" width="3" height="7" rx="1.5"/>
  <path d="M4 8c0 2.2 1.8 4 4 4s4-1.8 4-4M8 12v2"/>
</symbol>

<symbol id="i-x" viewBox="0 0 16 16">
  <path d="M4 4l8 8M12 4l-8 8"/>
</symbol>

<symbol id="i-chevron" viewBox="0 0 16 16">
  <path d="M4 6l4 4 4-4"/>
</symbol>

<symbol id="i-download" viewBox="0 0 16 16">
  <path d="M8 2v8M4.5 7L8 10.5 11.5 7M3 13h10"/>
</symbol>

<symbol id="i-info" viewBox="0 0 16 16">
  <circle cx="8" cy="8" r="6"/>
  <path d="M8 7.5V11.5" stroke-linecap="round"/>
  <circle cx="8" cy="5.2" r="0.7" fill="currentColor" stroke="none"/>
</symbol>

<symbol id="i-link" viewBox="0 0 16 16">
  <path d="M7 5h-1a3 3 0 0 0 0 6h1M9 5h1a3 3 0 0 1 0 6h-1M6 8h4"/>
</symbol>

<symbol id="i-unlink" viewBox="0 0 16 16">
  <path d="M7 5h-1a3 3 0 0 0 0 6h1M9 5h1a3 3 0 0 1 0 6h-1"/>
  <path d="M3 3l10 10" stroke="currentColor"/>
</symbol>

</defs></svg>`;

export function injectIconSprite() {
  if (document.getElementById("zound-icons")) return;
  const wrap = document.createElement("div");
  wrap.id = "zound-icons";
  wrap.style.cssText = "position:absolute;width:0;height:0;overflow:hidden;";
  wrap.setAttribute("aria-hidden", "true");
  wrap.innerHTML = SPRITE;
  document.body.appendChild(wrap);
}

/// Возвращает HTML-строку с <svg><use href="#…"/></svg>. Главное преимущество
/// перед `document.createElement` — можно вшивать в innerHTML-шаблон, не ломая
/// текущую сборку DOM.
export function ic(name, cls = "icon") {
  return `<svg class="${cls}" aria-hidden="true"><use href="#${name}"/></svg>`;
}

/// Иконка по типу/флагам устройства — для status-индикатора в списке.
export function deviceIcon(d) {
  if (!d) return "i-speaker";
  if (d.isInputOnly) return "i-mic";
  const name = (d.name || "").toLowerCase();
  if (/headphone|airpod|wh-|wf-|earbud|buds/.test(name)) return "i-headphones";
  if (/bluetooth|bt|airpods/.test(name)) return "i-bluetooth";
  return "i-speaker";
}
