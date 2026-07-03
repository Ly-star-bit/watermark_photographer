import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/**
 * 安全订阅一个异步返回"取消订阅函数"的注册调用（如 Tauri `listen()`），
 * 直接作为 `useEffect` 的返回值使用。
 *
 * 背景：`useEffect(() => { let unlisten; fn().then(f => unlisten = f); return () => unlisten?.() }, [])`
 * 这种写法在 React StrictMode（开发模式）下会出 bug——StrictMode 会立即把一次 mount 的
 * effect 执行"挂载→清理→再挂载"一遍，而 `listen()` 是异步的，清理函数执行时 promise
 * 往往还没 resolve，`unlisten` 仍是 undefined，清理等于没执行，导致第一次注册的监听
 * 永远没被取消，最终同一个事件会被两个监听器各收到一次（表现为收到"重复"的事件）。
 * 这里用 `cancelled` 标记：promise resolve 时如果已经被清理过，立即调用刚拿到的
 * unlisten 撤销这次注册，从而保证任意时刻只有一个有效监听。
 */
export function subscribeAsync(register: () => Promise<() => void>): () => void {
  let cancelled = false;
  let unlisten: (() => void) | undefined;
  register().then((fn) => {
    if (cancelled) {
      fn();
    } else {
      unlisten = fn;
    }
  });
  return () => {
    cancelled = true;
    unlisten?.();
  };
}
