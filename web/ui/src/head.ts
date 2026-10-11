// Inline, render-blocking on purpose: sets theme and language before first paint.
// Dark is the default; light is stored only when chosen.
export const bootScript = `(function(){var d=document.documentElement;try{var t=localStorage.getItem("skopli.theme");if(t==="light")d.dataset.theme="light";d.dataset.lang=localStorage.getItem("skopli.lang")||"ts";}catch(e){d.dataset.lang="ts";}})();`;
