import type { RequirementPriority, RequirementStatus } from "../../data";

export const priorityColor: Record<RequirementPriority, string> = {
  P0: "red",
  P1: "gold",
  P2: "green",
};

export const statusMeta: Record<RequirementStatus, { label: string; color: string }> = {
  todo: { label: "待开始", color: "default" },
  doing: { label: "进行中", color: "blue" },
  review: { label: "待评审", color: "gold" },
  done: { label: "已完成", color: "green" },
};
