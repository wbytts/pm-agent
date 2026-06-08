import { invoke } from "@tauri-apps/api/core";
import type {
  Project,
  ProjectVersion,
  Requirement,
  RequirementPriority,
} from "./data";

export interface ProjectDraft {
  name: string;
  description: string;
  dueDate: string;
  owner: string;
  members: number;
}

export interface RequirementDraft {
  title: string;
  projectId: string;
  versionId: string;
  priority: RequirementPriority;
  type: Requirement["type"];
  assignee: string;
  dueDate: string;
  description: string;
}

export interface ProjectVersionDraft {
  projectId: string;
  name: string;
  description: string;
}

export function initializeProjectDatabase() {
  return invoke<void>("project_initialize_database");
}

export function listProjects() {
  return invoke<Project[]>("project_list_projects");
}

export function listProjectVersions() {
  return invoke<ProjectVersion[]>("project_list_versions");
}

export function listRequirements() {
  return invoke<Requirement[]>("project_list_requirements");
}

export function createProject(draft: ProjectDraft) {
  return invoke<string>("project_create_project", { draft });
}

export function createProjectVersion(draft: ProjectVersionDraft) {
  return invoke<string>("project_create_version", { draft });
}

export function createRequirement(draft: RequirementDraft) {
  return invoke<string>("project_create_requirement", { draft });
}
