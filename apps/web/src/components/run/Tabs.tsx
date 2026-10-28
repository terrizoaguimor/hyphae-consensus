"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The run workspace's tabs: the WAI-ARIA tabs pattern (role="tablist",
// role="tab" with aria-selected and aria-controls, one panel), automatic
// activation, a roving tabindex with Left/Right/Home/End, styled by classes
// alone. Astryx's TabList injects a <style> element at runtime through its
// overflow layer, which the portal's hash-only CSP (style-src 'self' plus
// the hashes of the exported HTML) blocks, so it is not used here.

import { type KeyboardEvent, useRef } from "react";

export type TabItem<V extends string> = {
  value: V;
  label: string;
};

type TabsProps<V extends string> = {
  /** Accessible name of the tab list. */
  label: string;
  value: V;
  onChange: (value: V) => void;
  items: ReadonlyArray<TabItem<V>>;
  /** Id of the single panel the tabs control. */
  panelId: string;
};

export function tabId(panelId: string, value: string): string {
  return `${panelId}-tab-${value}`;
}

export function Tabs<V extends string>({ label, value, onChange, items, panelId }: TabsProps<V>) {
  const buttons = useRef(new Map<string, HTMLButtonElement>());

  const activate = (next: TabItem<V> | undefined) => {
    if (next === undefined) {
      return;
    }
    onChange(next.value);
    buttons.current.get(next.value)?.focus();
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const index = items.findIndex((item) => item.value === value);
    if (index === -1) {
      return;
    }
    switch (event.key) {
      case "ArrowRight":
        activate(items[(index + 1) % items.length]);
        break;
      case "ArrowLeft":
        activate(items[(index - 1 + items.length) % items.length]);
        break;
      case "Home":
        activate(items[0]);
        break;
      case "End":
        activate(items[items.length - 1]);
        break;
      default:
        return;
    }
    event.preventDefault();
  };

  return (
    <div role="tablist" aria-label={label} className="hc-tabs" onKeyDown={onKeyDown}>
      {items.map((item) => {
        const selected = item.value === value;
        return (
          <button
            key={item.value}
            ref={(element) => {
              if (element === null) {
                buttons.current.delete(item.value);
              } else {
                buttons.current.set(item.value, element);
              }
            }}
            type="button"
            role="tab"
            id={tabId(panelId, item.value)}
            className="hc-tab"
            aria-selected={selected}
            aria-controls={panelId}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(item.value)}
          >
            {item.label}
          </button>
        );
      })}
    </div>
  );
}
