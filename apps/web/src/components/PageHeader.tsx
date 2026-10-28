// SPDX-License-Identifier: Apache-2.0
//
// The top of every page: its single <h1>, focusable by script only
// (tabindex="-1") so RouteFocus can move focus there after a navigation.

import type { ReactNode } from "react";

type PageHeaderProps = {
  title: string;
  lead?: ReactNode;
  /** Shown above the title, e.g. a status badge. */
  eyebrow?: ReactNode;
  children?: ReactNode;
};

export function PageHeader({ title, lead, eyebrow, children }: PageHeaderProps) {
  return (
    <header className="hc-page-header">
      {eyebrow}
      <h1 tabIndex={-1} className="hc-page-title">
        {title}
      </h1>
      {lead === undefined ? null : <p className="hc-lead">{lead}</p>}
      {children}
    </header>
  );
}
