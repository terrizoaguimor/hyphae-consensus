"use client";
// SPDX-License-Identifier: Apache-2.0
//
// A modal dialog on the native <dialog> element (showModal): the rest of the
// page becomes inert, Escape closes it, and it has dialog semantics. On top of
// that: Tab and Shift+Tab wrap inside the dialog (a strict focus trap, so
// focus never leaves for the browser chrome), a click on the backdrop closes
// it, and focus returns to the element that opened it, or to the page heading
// when that element is gone.

import {
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
  type RefObject,
  type SyntheticEvent,
  useEffect,
  useRef,
} from "react";

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), summary, [tabindex]:not([tabindex="-1"])';

function isRadio(element: Element): element is HTMLInputElement {
  return element instanceof HTMLInputElement && element.type === "radio";
}

/**
 * The Tab stops inside `root`, in order. A named radio group (the drawer's
 * theme control) is one stop, as the browser makes it: its checked radio,
 * or its first one while none is checked. Otherwise Tab from the checked
 * radio would leave the dialog before the trap saw the last stop.
 */
function focusableIn(root: HTMLElement): HTMLElement[] {
  const candidates = [
    ...root.querySelectorAll<HTMLElement>(FOCUSABLE),
  ].filter(
    (element) =>
      !element.closest("[inert]") && element.getClientRects().length > 0,
  );
  return candidates.filter((element) => {
    if (!isRadio(element) || element.name === "") {
      return true;
    }
    const group = candidates.filter(
      (other): other is HTMLInputElement =>
        isRadio(other) && other.name === element.name,
    );
    return element === (group.find((radio) => radio.checked) ?? group[0]);
  });
}

/** The page's main heading, the fallback focus target. */
export function focusPageHeading(): void {
  const heading = document.querySelector<HTMLElement>("main h1");
  (heading ?? document.getElementById("main"))?.focus();
}

type ModalProps = {
  isOpen: boolean;
  /** Escape, a backdrop click or the dialog's own close controls. */
  onClose: () => void;
  labelledBy: string;
  describedBy?: string;
  className: string;
  /** Receives focus when the dialog opens; defaults to the first control. */
  initialFocus?: RefObject<HTMLElement | null>;
  children: ReactNode;
};

export function Modal({
  isOpen,
  onClose,
  labelledBy,
  describedBy,
  className,
  initialFocus,
  children,
}: ModalProps) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const returnFocusRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (dialog === null) {
      return;
    }
    if (isOpen && !dialog.open) {
      const active = document.activeElement;
      returnFocusRef.current = active instanceof HTMLElement ? active : null;
      dialog.showModal();
      const target = initialFocus?.current ?? focusableIn(dialog)[0] ?? dialog;
      target.focus();
    } else if (!isOpen) {
      if (dialog.open) {
        dialog.close();
      }
      const target = returnFocusRef.current;
      returnFocusRef.current = null;
      if (target !== null) {
        if (target.isConnected) {
          target.focus();
        } else {
          focusPageHeading();
        }
      }
    }
  }, [isOpen, initialFocus]);

  // The browser may close the dialog on its own (Escape without a cancelable
  // cancel event): keep the state in step.
  const onNativeClose = () => {
    if (isOpen) {
      onClose();
    }
  };

  const onCancel = (event: SyntheticEvent<HTMLDialogElement>) => {
    event.preventDefault();
    onClose();
  };

  // A click whose target is the <dialog> itself landed on the backdrop: the
  // content fills the dialog box, so clicks inside never target it.
  const onClick = (event: MouseEvent<HTMLDialogElement>) => {
    if (event.target === event.currentTarget) {
      onClose();
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDialogElement>) => {
    if (event.key !== "Tab") {
      return;
    }
    const dialog = event.currentTarget;
    const focusable = focusableIn(dialog);
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (first === undefined || last === undefined) {
      event.preventDefault();
      return;
    }
    const active = document.activeElement;
    if (event.shiftKey) {
      if (active === first || !dialog.contains(active) || active === dialog) {
        event.preventDefault();
        last.focus();
      }
    } else if (active === last || !dialog.contains(active)) {
      event.preventDefault();
      first.focus();
    }
  };

  return (
    <dialog
      ref={dialogRef}
      className={className}
      // Only while open: the labelling elements may render only then.
      aria-labelledby={isOpen ? labelledBy : undefined}
      aria-describedby={isOpen ? describedBy : undefined}
      onCancel={onCancel}
      onClose={onNativeClose}
      onClick={onClick}
      onKeyDown={onKeyDown}
    >
      <div className="hc-dialog-content">{children}</div>
    </dialog>
  );
}
