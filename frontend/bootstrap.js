(async () => {
  try {
    const base = new URL('.', import.meta.url);

    const jsUrl = new URL('./frontend.js', base);
    const wasmUrl = new URL('./frontend_bg.wasm', base);

    const mod = await import(jsUrl.href);

    const init = mod.default;

    const wasm = await init({ module_or_path: wasmUrl.href });

    window.wasmBindings = mod;
    dispatchEvent(new CustomEvent('TrunkApplicationStarted', { detail: { wasm } }));
  } catch (err) {
    console.error('Tuliprox WASM bootstrap failed:', err);
  }
})();
