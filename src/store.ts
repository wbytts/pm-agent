import { create } from "zustand";
import type { Project, ProjectVersion, Requirement, RequirementPriority, RequirementStatus, ViewKey } from "./data";
import {
  createProject,
  createProjectVersion,
  createRequirement,
  initializeProjectDatabase,
  listProjectVersions,
  listProjects,
  listRequirements,
  type ProjectDraft,
  type ProjectVersionDraft,
  type RequirementDraft,
} from "./projectRepository";

interface WorkspaceState {
  activeView: ViewKey;
  projects: Project[];
  projectVersions: ProjectVersion[];
  requirements: Requirement[];
  loading: boolean;
  databaseError: string | null;
  selectedRequirementId: string | null;
  selectedProjectId: string | null;
  selectedVersionId: string | null;
  requirementSearch: string;
  projectFilter: string;
  versionFilter: string;
  priorityFilter: "all" | RequirementPriority;
  statusFilter: "all" | RequirementStatus;
  detailTab: string;
  knowledgeTab: string;
  generated: boolean;
  loadWorkspace: () => Promise<void>;
  createProject: (draft: ProjectDraft) => Promise<void>;
  createProjectVersion: (draft: ProjectVersionDraft) => Promise<void>;
  createRequirement: (draft: RequirementDraft) => Promise<void>;
  setActiveView: (view: ViewKey) => void;
  openProjectRequirements: (projectId: string, versionId?: string) => void;
  openRequirement: (id: string) => void;
  closeRequirement: () => void;
  setRequirementSearch: (value: string) => void;
  setProjectFilter: (value: string) => void;
  setVersionFilter: (value: string) => void;
  setPriorityFilter: (value: "all" | RequirementPriority) => void;
  setStatusFilter: (value: "all" | RequirementStatus) => void;
  setDetailTab: (tab: string) => void;
  setKnowledgeTab: (tab: string) => void;
  showGenerated: () => void;
}

export const useWorkspaceStore = create<WorkspaceState>((set, get) => ({
  activeView: "requirements",
  projects: [],
  projectVersions: [],
  requirements: [],
  loading: false,
  databaseError: null,
  selectedRequirementId: null,
  selectedProjectId: null,
  selectedVersionId: null,
  requirementSearch: "",
  projectFilter: "all",
  versionFilter: "all",
  priorityFilter: "all",
  statusFilter: "all",
  detailTab: "描述",
  knowledgeTab: "project",
  generated: false,
  loadWorkspace: async () => {
    set({ loading: true, databaseError: null });
    try {
      await initializeProjectDatabase();
      const [projects, projectVersions, requirements] = await Promise.all([
        listProjects(),
        listProjectVersions(),
        listRequirements(),
      ]);
      set({ projects, projectVersions, requirements, loading: false });
    } catch (error) {
      const databaseError = error instanceof Error ? error.message : String(error);
      set({ databaseError, loading: false });
    }
  },
  createProject: async (draft) => {
    await createProject(draft);
    await get().loadWorkspace();
  },
  createProjectVersion: async (draft) => {
    await createProjectVersion(draft);
    await get().loadWorkspace();
  },
  createRequirement: async (draft) => {
    const id = await createRequirement(draft);
    await get().loadWorkspace();
    set({ activeView: "requirements", selectedRequirementId: id });
  },
  setActiveView: (view) =>
    set({
      activeView: view,
      selectedRequirementId: view === "requirements" ? get().selectedRequirementId : null,
    }),
  openProjectRequirements: (projectId, versionId = "all") =>
    set({
      activeView: "requirements",
      selectedProjectId: projectId,
      selectedVersionId: versionId === "all" ? null : versionId,
      projectFilter: projectId,
      versionFilter: versionId,
      selectedRequirementId: null,
    }),
  openRequirement: (id) => set({ activeView: "requirements", selectedRequirementId: id }),
  closeRequirement: () => set({ selectedRequirementId: null }),
  setRequirementSearch: (value) => set({ requirementSearch: value }),
  setProjectFilter: (value) =>
    set({ projectFilter: value, versionFilter: "all", selectedProjectId: value === "all" ? null : value, selectedVersionId: null }),
  setVersionFilter: (value) => set({ versionFilter: value, selectedVersionId: value === "all" ? null : value }),
  setPriorityFilter: (value) => set({ priorityFilter: value }),
  setStatusFilter: (value) => set({ statusFilter: value }),
  setDetailTab: (tab) => set({ detailTab: tab }),
  setKnowledgeTab: (tab) => set({ knowledgeTab: tab }),
  showGenerated: () => set({ generated: true }),
}));
