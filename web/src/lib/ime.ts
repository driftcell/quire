import { useCallback, useRef } from "react";
import type { CompositionEvent, KeyboardEvent } from "react";

/**
 * Duration (in ms) after `compositionend` during which a trailing `Enter`
 * `keydown` must still be treated as the IME commit keystroke rather than a
 * form submission.
 *
 * Per MDN (`Element: keydown_event`) and W3C UI Events:
 * - Safari/WebKit and several macOS/Windows CJK IMEs (especially when typing
 *   raw English letters inside Chinese IME mode and pressing Enter to commit)
 *   fire `compositionend` BEFORE the `Enter` `keydown` event, so
 *   `event.nativeEvent.isComposing` is already `false` when `onKeyDown` runs.
 * - Checking `keyCode === 229` (`VK_PROCESSKEY`), `key === "Process"`, an
 *   explicit `composingRef` flag, AND a post-`compositionend` time window
 *   eliminates premature Enter submissions across Chrome, Safari, Firefox, and
 *   all major Chinese/Japanese/Korean IMEs.
 */
export const IME_COMPOSITION_END_GUARD_MS = 80;

export interface NativeImeKeyEventLike {
  key: string;
  nativeEvent: {
    isComposing?: boolean;
    keyCode?: number;
  };
}

export function isNativeImeKeyEvent(event: NativeImeKeyEventLike): boolean {
  const native = event.nativeEvent;
  if (native.isComposing || event.key === "Process") {
    return true;
  }
  return native.keyCode === 229;
}

export function useImeCompositionGuard<
  T extends HTMLElement = HTMLTextAreaElement,
>() {
  const composingRef = useRef(false);
  const lastCompositionEndMsRef = useRef(-Infinity);

  const onCompositionStart = useCallback(
    (_event?: CompositionEvent<T>, nowMs = Date.now()) => {
      void nowMs;
      composingRef.current = true;
    },
    [],
  );

  const onCompositionEnd = useCallback(
    (_event?: CompositionEvent<T>, nowMs = Date.now()) => {
      composingRef.current = false;
      lastCompositionEndMsRef.current = nowMs;
    },
    [],
  );

  const isComposingOrJustEnded = useCallback(
    (event: KeyboardEvent<T>, nowMs = Date.now()): boolean => {
      if (composingRef.current || isNativeImeKeyEvent(event)) {
        return true;
      }
      const deltaMs = nowMs - lastCompositionEndMsRef.current;
      return deltaMs >= 0 && deltaMs < IME_COMPOSITION_END_GUARD_MS;
    },
    [],
  );

  const isSubmitEnter = useCallback(
    (event: KeyboardEvent<T>, nowMs = Date.now()): boolean => {
      if (event.key !== "Enter" || event.shiftKey) {
        return false;
      }
      return !isComposingOrJustEnded(event, nowMs);
    },
    [isComposingOrJustEnded],
  );

  return {
    onCompositionStart,
    onCompositionEnd,
    isComposingOrJustEnded,
    isSubmitEnter,
  };
}
