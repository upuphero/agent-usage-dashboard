export function isTauriRuntime(): boolean {
  return '__TAURI_INTERNALS__' in globalThis;
}
