/** How a core's conversations are organized: workspaces and projects. */
import {
  projectAnswerSchema,
  projectsSchema,
  workspaceAnswerSchema,
  workspacesSchema,
  type ChangeProject,
  type ChangeWorkspace,
  type CreateProject,
  type CreateWorkspace,
  type ProjectDto,
  type WorkspaceDto,
} from '@jush-agent/protocol'
import { segment, type CoreRoutes } from './routes'

export interface Workspaces {
  list(): Promise<WorkspaceDto[]>
  create(workspace: CreateWorkspace): Promise<WorkspaceDto>
  update(id: string, patch: ChangeWorkspace): Promise<WorkspaceDto>
  remove(id: string): Promise<void>
  /** Orders the workspaces, each once. */
  order(ids: string[]): Promise<void>
  /** Moves conversations and projects into the workspace `id`. */
  move(id: string, moved: { conversations?: string[]; projects?: string[] }): Promise<void>
}

export function workspaces(routes: CoreRoutes): Workspaces {
  const one = (id: string) => `/workspaces/${segment(id)}`
  return {
    list: async () => (await routes.json('GET', '/workspaces', workspacesSchema)).workspaces,
    create: async (workspace) => (await routes.json('POST', '/workspaces', workspaceAnswerSchema, workspace)).workspace,
    update: async (id, patch) => (await routes.json('PATCH', one(id), workspaceAnswerSchema, patch)).workspace,
    remove: (id) => routes.send('DELETE', one(id)),
    order: (ids) => routes.send('PUT', '/workspaces/order', { ids }),
    move: (id, moved) => routes.send('POST', `${one(id)}/moves`, { conversations: moved.conversations ?? [], projects: moved.projects ?? [] }),
  }
}

export interface Projects {
  list(): Promise<ProjectDto[]>
  create(project: CreateProject): Promise<ProjectDto>
  update(id: string, patch: ChangeProject): Promise<ProjectDto>
  remove(id: string): Promise<void>
}

export function projects(routes: CoreRoutes): Projects {
  const one = (id: string) => `/projects/${segment(id)}`
  return {
    list: async () => (await routes.json('GET', '/projects', projectsSchema)).projects,
    create: async (project) => (await routes.json('POST', '/projects', projectAnswerSchema, project)).project,
    update: async (id, patch) => (await routes.json('PATCH', one(id), projectAnswerSchema, patch)).project,
    remove: (id) => routes.send('DELETE', one(id)),
  }
}
