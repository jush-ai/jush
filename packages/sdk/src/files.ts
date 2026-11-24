/**
 * The files of a conversation's devices and the working tree's changes, as
 * the conversation's Files and Changes show them.
 */
import {
  changeSidesSchema,
  createdDirectorySchema,
  directorySchema,
  fileTextSchema,
  gitBranchesSchema,
  gitCommitSchema,
  gitLogSchema,
  gitRepositoriesSchema,
  listedDirectoriesSchema,
  searchLineSchema,
  workingTreeChangesSchema,
  type ChangeSides,
  type CreatedDirectory,
  type Directory,
  type FileText,
  type GitBranches,
  type GitCommit,
  type GitLog,
  type GitRepositories,
  type ListedDirectories,
  type SearchLine,
  type SearchRequest,
  type WorkingTreeChanges,
} from '@jush-agent/protocol'
import { query, segment, type CoreRoutes } from './routes'

export interface ConversationFiles {
  /** The entries of the directory `path` on the conversation's primary device, its working directory when absent, or on its attached `device`. */
  list(options?: { path?: string; device?: string }): Promise<Directory>
  /** Several directories' entries, read together. */
  lists(paths: string[]): Promise<ListedDirectories>
  /** A file's text, by its path from the working directory or an absolute one. */
  read(path: string): Promise<FileText>
  /** A file's bytes. */
  readBytes(path: string): Promise<Uint8Array>
  /** Writes `bytes` as the file `path`, whole or not at all; `replace` overwrites one that is there. */
  write(path: string, bytes: Uint8Array | ArrayBuffer | Blob, options?: { replace?: boolean }): Promise<void>
  /** Makes the directory `path`, with its parents, on the primary device or the attached `device`. */
  makeDirectory(path: string, options?: { device?: string }): Promise<CreatedDirectory>
  /** Deletes a file, or a directory with what is in it, by an absolute path or one under `~/`. */
  remove(path: string): Promise<void>
  /** The lines of the files' names and text that match, as the device finds them. */
  search(request: SearchRequest, signal?: AbortSignal): AsyncGenerator<SearchLine>
}

export interface ConversationChanges {
  /** The working tree's uncommitted changes, of `directory`'s tree when named. */
  status(options?: { directory?: string }): Promise<WorkingTreeChanges>
  /** A changed file's two sides; `commit` compares that commit with its parent. */
  file(path: string, options?: { from?: string; directory?: string; commit?: string }): Promise<ChangeSides>
  /** A file's bytes as the last commit, or `commit`, holds them. */
  committed(path: string, options?: { directory?: string; commit?: string }): Promise<Uint8Array>
  branches(options?: { directory?: string }): Promise<GitBranches>
  /** Which of `paths` are working trees, and their roots. */
  repositories(paths: string[]): Promise<GitRepositories>
  /** A page of a branch's history, HEAD's unless named, newest first, after the commit `after` when named. */
  log(options?: { directory?: string; branch?: string; after?: string }): Promise<GitLog>
  /** The commit `commit`: its message, its author and the files it changed. */
  show(commit: string, options?: { directory?: string }): Promise<GitCommit>
}

/** The files of a device, by absolute paths. */
export interface DeviceFiles {
  /** The entries of the directory `path`, the device's home when absent. */
  list(path?: string): Promise<Directory>
  /** Makes the directory `path`, with its parents. */
  makeDirectory(path: string): Promise<CreatedDirectory>
}

export function conversationFiles(routes: CoreRoutes, id: string): ConversationFiles {
  const base = `/conversations/${segment(id)}`
  return {
    list: (options = {}) => routes.json(
      'GET',
      `${options.device === undefined ? `${base}/fs` : `${base}/devices/${segment(options.device)}/fs`}${query({ path: options.path })}`,
      directorySchema,
    ),
    lists: (paths) => routes.json('POST', `${base}/fs/lists`, listedDirectoriesSchema, { paths }),
    read: (path) => routes.json('GET', `${base}/fs/file${query({ path })}`, fileTextSchema),
    readBytes: (path) => routes.bytes('GET', `${base}/fs/raw${query({ path })}`),
    write: (path, bytes, options = {}) => routes.send(
      'PUT',
      `${base}/fs/raw${query({ path, replace: options.replace })}`,
      bytes instanceof Blob ? bytes : new Blob([bytes]),
    ),
    makeDirectory: (path, options = {}) => routes.json(
      'POST',
      options.device === undefined ? `${base}/fs` : `${base}/devices/${segment(options.device)}/fs`,
      createdDirectorySchema,
      { path },
    ),
    remove: (path) => routes.send('DELETE', `${base}/fs${query({ path })}`),
    search: (request, signal) => routes.lines('POST', `${base}/fs/search`, searchLineSchema, request, signal),
  }
}

export function conversationChanges(routes: CoreRoutes, id: string): ConversationChanges {
  const base = `/conversations/${segment(id)}/changes`
  return {
    status: (options = {}) => routes.json('GET', `${base}${query({ directory: options.directory })}`, workingTreeChangesSchema),
    file: (path, options = {}) => routes.json('GET', `${base}/file${query({ path, ...options })}`, changeSidesSchema),
    committed: (path, options = {}) => routes.bytes('GET', `${base}/raw${query({ path, ...options })}`),
    branches: (options = {}) => routes.json('GET', `${base}/branches${query({ directory: options.directory })}`, gitBranchesSchema),
    repositories: (paths) => routes.json('POST', `${base}/repositories`, gitRepositoriesSchema, { paths }),
    log: (options = {}) => routes.json('GET', `${base}/log${query({ ...options })}`, gitLogSchema),
    show: (commit, options = {}) => routes.json('GET', `${base}/commit${query({ commit, directory: options.directory })}`, gitCommitSchema),
  }
}

export function deviceFiles(routes: CoreRoutes, id: string): DeviceFiles {
  const base = `/devices/${segment(id)}/fs`
  return {
    list: (path) => routes.json('GET', `${base}${query({ path })}`, directorySchema),
    makeDirectory: (path) => routes.json('POST', base, createdDirectorySchema, { path }),
  }
}
