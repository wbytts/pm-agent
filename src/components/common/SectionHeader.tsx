import type { ViewKey } from "../../data";
import { useWorkspaceStore } from "../../store";

export function SectionHeader({ title, action, view }: { title: string; action?: string; view?: ViewKey }) {
  const setActiveView = useWorkspaceStore((state) => state.setActiveView);

  return (
    <div className="mb-3 flex items-center justify-between">
      <h2 className="m-0 text-sm font-bold">{title}</h2>
      {action && view && (
        <button
          className="border-0 bg-transparent p-0 text-[11px] text-[#667085] hover:text-[#2563eb]"
          onClick={() => setActiveView(view)}
        >
          {action}
        </button>
      )}
    </div>
  );
}
