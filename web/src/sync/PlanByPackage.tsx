import {
  Button,
  Popover,
  PopoverContent,
  PopoverTrigger,
  Search,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Table,
  TableBody,
  TableCell,
  TableFrame,
  TableHead,
  TableHeader,
  TableRow,
} from "@momoi-labs/kiso-react";
import { Minus, Plus, RefreshCw } from "lucide-react";
import { useState } from "react";

import type { SyncHost } from "../api";
import {
  byPackage,
  type Change,
  hostsLabel,
  type PackageRow,
} from "./byPackage";
import { plural } from "./sync";

const ALL = "*";

const sections: { change: Change; title: string; icon: typeof Plus }[] = [
  { change: "install", title: "Install", icon: Plus },
  { change: "update", title: "Update", icon: RefreshCw },
  { change: "remove", title: "Remove", icon: Minus },
];

const reasons = {
  in_sync: "In sync",
  offline: "Offline",
  failed: "Plan failed",
  planning: "Waiting for a plan",
} as const;

/** The plans of a sync by package: what installs, updates, and removes,
 *  and on which hosts. A search and a host filter narrow the rows. */
export function PlanByPackage({ hosts }: { hosts: SyncHost[] }) {
  const [query, setQuery] = useState("");
  const [only, setOnly] = useState(ALL);
  const all = byPackage(hosts);
  const picked =
    only === ALL ? hosts : hosts.filter((host) => host.id === only);
  const plan = only === ALL ? all : byPackage(picked);
  const typed = query.trim().toLowerCase();
  const match = (row: PackageRow) =>
    !typed || row.name.toLowerCase().includes(typed);
  const shown = sections
    .map((section) => ({
      ...section,
      rows: plan[section.change].filter(match),
    }))
    .filter((section) => section.rows.length > 0);
  const byReason = Object.entries(reasons)
    .map(([state, label]) => ({
      label,
      names: all.unplanned
        .filter((entry) => entry.state === state)
        .map((entry) => entry.host.name),
    }))
    .filter((entry) => entry.names.length > 0);

  return (
    <div className="stack">
      {all.planned.length > 0 && (
        <div className="plan-toolbar">
          <Search
            aria-label="Filter packages"
            placeholder="Filter by package"
            containerClassName="plan-search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
          <Select value={only} onValueChange={setOnly}>
            <SelectTrigger aria-label="Show the plan of" className="plan-host">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={ALL}>All hosts</SelectItem>
              {all.planned.map((host) => (
                <SelectItem key={host.id} value={host.id}>
                  Only {host.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      )}
      {shown.length === 0 && all.planned.length > 0 && (
        <p className="muted" role="status">
          {typed
            ? `No package matches "${query.trim()}".`
            : "This host has nothing to change."}
        </p>
      )}
      {shown.map((section) => (
        <section key={section.change} className="stack-sm">
          <h3 className="t-caps plan-heading" id={`plan-${section.change}`}>
            <section.icon className="icon" aria-hidden="true" />
            {section.title} ({section.rows.length})
          </h3>
          <TableFrame className="plan-table" data-shadow-style="none">
            <Table density="compact" aria-labelledby={`plan-${section.change}`}>
              <colgroup>
                <col />
                <col className="plan-col-manager" />
                {section.change !== "remove" && (
                  <col className="plan-col-version" />
                )}
                <col className="plan-col-hosts" />
              </colgroup>
              <TableHeader>
                <TableRow>
                  <TableHead>Package</TableHead>
                  <TableHead>Manager</TableHead>
                  {section.change === "install" && (
                    <TableHead>Version</TableHead>
                  )}
                  {section.change === "update" && (
                    <TableHead>From → to</TableHead>
                  )}
                  <TableHead>Hosts</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {section.rows.map((row) => (
                  <TableRow key={row.key} data-change={section.change}>
                    <TableCell className="plan-name mono">{row.name}</TableCell>
                    <TableCell data-label="Manager">{row.manager}</TableCell>
                    {section.change === "install" && (
                      <TableCell className="mono" data-label="Version">
                        {row.to ?? "latest"}
                      </TableCell>
                    )}
                    {section.change === "update" && (
                      <TableCell className="mono" data-label="From → to">
                        {row.from ?? "?"} → {row.to ?? "?"}
                      </TableCell>
                    )}
                    <TableCell data-label="Hosts">
                      <Hosts row={row} planned={plan.planned.length} />
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </TableFrame>
        </section>
      ))}
      {(plan.unchanged > 0 || plan.unknown > 0) && (
        <p className="muted">
          {[
            plan.unchanged > 0 && `${plural(plan.unchanged, "item")} unchanged`,
            plan.unknown > 0 &&
              `${plural(plan.unknown, "item")} the agent could not plan`,
          ]
            .filter(Boolean)
            .join(", ")}
          .
        </p>
      )}
      {byReason.length > 0 && (
        <p className="muted plan-unplanned">
          No plan to sign:{" "}
          {byReason
            .map((entry) => `${entry.label}: ${entry.names.join(", ")}`)
            .join(". ")}
          .
        </p>
      )}
    </div>
  );
}

/** The hosts of a row. When there are many, a button opens the full list. */
function Hosts({ row, planned }: { row: PackageRow; planned: number }) {
  const { text, more } = hostsLabel(row.hosts, planned);
  if (more === 0) return <>{text}</>;
  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          className="btn-link plan-more"
          aria-label={`${text}: show all ${row.hosts.length} hosts for ${row.name}`}
        >
          {text}
        </Button>
      </PopoverTrigger>
      <PopoverContent align="start">
        <p className="t-caps">{plural(row.hosts.length, "host")}</p>
        <ul className="plan-host-list">
          {row.hosts.map((name) => (
            <li key={name}>{name}</li>
          ))}
        </ul>
      </PopoverContent>
    </Popover>
  );
}
