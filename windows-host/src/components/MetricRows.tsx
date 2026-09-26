import React from "react";

interface MetricRowsProps {
  rows: [string, string][];
}

export function MetricRows({ rows }: MetricRowsProps) {
  return (
    <dl>
      {rows.map(([label, value]) => (
        <React.Fragment key={label}>
          <dt>{label}</dt>
          <dd>{value}</dd>
        </React.Fragment>
      ))}
    </dl>
  );
}
