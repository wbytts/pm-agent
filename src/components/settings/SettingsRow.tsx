import { Card } from "antd";
import type { ReactNode } from "react";

export function SettingsSection({ title, children }: { title: string; children: ReactNode }) {
  return (
    <Card size="small" title={title} className="desktop-card">
      <div className="space-y-2">{children}</div>
    </Card>
  );
}

export function SettingRow({ label, desc, control }: { label: string; desc?: string; control: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-3 py-1">
      <div>
        <div className="text-xs font-medium">{label}</div>
        {desc && <div className="mt-0.5 text-[11px] text-[#667085]">{desc}</div>}
      </div>
      <div>{control}</div>
    </div>
  );
}
