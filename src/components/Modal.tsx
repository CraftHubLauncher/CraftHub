import { useEffect, useId, useRef, type ReactNode } from "react";
import { X } from "lucide-react";

export function Modal({
  title,
  onClose,
  children,
  footer,
  variant = "dialog",
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  variant?: "dialog" | "drawer";
}) {
  const ref = useRef<HTMLDivElement>(null);
  const titleId = useId();

  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const first = ref.current?.querySelector<HTMLElement>(
      "button:not([disabled]), [href], input, select, textarea",
    );
    first?.focus();
    const onKey = (e: KeyboardEvent) => {
      // With stacked dialogs only the one containing focus (the topmost) reacts.
      if (!ref.current || !ref.current.contains(document.activeElement)) return;
      if (e.key === "Escape") {
        onClose();
        return;
      }
      if (e.key !== "Tab") return;
      const focusable = Array.from(
        ref.current.querySelectorAll<HTMLElement>(
          "button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea",
        ),
      );
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (!first || !last) return;
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      previous?.focus?.();
    };
  }, [onClose]);

  return (
    <div
      className={`overlay overlay-${variant}`}
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
    >
      <div ref={ref} className={variant} role="dialog" aria-modal="true" aria-labelledby={titleId}>
        <header className={`${variant}-header`}>
          <h2 id={titleId}>{title}</h2>
          <button className="icon-btn" onClick={onClose} aria-label="Close">
            <X size={18} />
          </button>
        </header>
        <div className={`${variant}-body`}>{children}</div>
        {footer && <footer className={`${variant}-footer`}>{footer}</footer>}
      </div>
    </div>
  );
}
