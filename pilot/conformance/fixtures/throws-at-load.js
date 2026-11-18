// Loaded by cross-origin-errors.html. Named by its ?name= parameter, it
// installs window[`${name}Timer`], which throws an Error from a timer, and
// throws an Error at load.
(() => {
  const name = new URL(document.currentScript.src).searchParams.get('name');
  window[`${name}Timer`] = () => setTimeout(() => {
    throw new Error(`${name} timer`);
  }, 0);
  throw new Error(`${name} at load`);
})();
