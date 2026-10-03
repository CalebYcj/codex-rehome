import { useEffect, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";
export default function Modal({
  title,
  children,
  onClose,
  busy = false,
}: {
  title: string;
  children: ReactNode;
  onClose: () => void;
  busy?: boolean;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const close = useRef(onClose);
  close.current = onClose;
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const root = document.getElementById("root");
    const wasInert = root?.hasAttribute("inert");
    root?.setAttribute("inert", "");
    ref.current?.querySelector<HTMLElement>("[data-autofocus]")?.focus();
    function key(event: KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        if (!busy) close.current();
      }
      if (event.key !== "Tab") return;
      const controls = [
        ...(ref.current?.querySelectorAll<HTMLElement>(
          "button:not(:disabled), input:not(:disabled), textarea:not(:disabled), a[href]",
        ) ?? []),
      ];
      const first = controls[0];
      const last = controls[controls.length - 1];
      if (!first) {
        event.preventDefault();
        ref.current?.focus();
        return;
      }
      if (
        event.shiftKey &&
        (document.activeElement === first ||
          !ref.current?.contains(document.activeElement))
      ) {
        event.preventDefault();
        last.focus();
      } else if (
        !event.shiftKey &&
        (document.activeElement === last ||
          !ref.current?.contains(document.activeElement))
      ) {
        event.preventDefault();
        first.focus();
      }
    }
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("keydown", key);
      if (!wasInert) root?.removeAttribute("inert");
      if (previous?.isConnected) previous.focus();
    };
  }, [busy]);
  return createPortal(
    <div className="modal-backdrop">
      <div
        className="modal"
        role="dialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
        ref={ref}
      >
        <h2>{title}</h2>
        {children}
      </div>
    </div>,
    document.body,
  );
}
