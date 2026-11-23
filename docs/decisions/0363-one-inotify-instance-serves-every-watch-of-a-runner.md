---
status: proposed
---

# One inotify instance serves every file system watch of a runner

## Problem

On Linux, each file system watch a runner starts is a notify watcher of its
own, and so an inotify instance of its own. A device whose project holds a
skills folder of 200 skills starts about 220 watches when a conversation on
it begins a turn: the project folder a few times over, the skills folder,
and each skill's folder. Each is an instance that watches one folder.

Linux limits the instances of one user, 128 by default
(`fs.inotify.max_user_instances`), across every process of that user. So:

- The runner reaches the limit and cannot watch the rest: its log says
  `subscription.unwatchable` with "No file descriptors available", and
  changes to those skills go unseen.
- Every other process of the same user that watches files, such as a
  development server, fails to start meanwhile.
- Several devices that run as one user on one host, such as containers with
  the same uid, exhaust the limit between them as soon as a few are active.

## Decision

On Linux, every watch of a runner's process shares one notify watcher, one
inotify instance holding one inotify watch per folder. A runner then takes
one instance, and one inotify watch for each folder it watches, however
many of its watches follow that folder. The folders count against the
watch limit (`fs.inotify.max_user_watches`),
8,192 on older kernels and sized by memory on newer ones: twenty runners
of one user that each watch the same 220 folders take 4,400 of 8,192, where
they took 4,400 instances of 128.

- The shared watcher is made with the first watch and closed with the last,
  so an idle runner holds no instance, as now.
- It keeps, for each folder, the watches that follow it. A folder is
  watched once, however many watches follow it, and is let go when the
  last of them stops.
- Each event goes to the watches that follow the folder it happened in, or
  the folder itself; a lost-events notice or a failure without a path goes
  to every watch. Each watch keeps judging what it receives by its own reach
  and its repository's ignore rules, as now.
- A watch that adds a folder asks the watcher for it again even when another
  watch follows it already: a folder removed and made again under its name
  is a new folder to inotify.
- What `Watches` (`crates/runner-device/src/watches.rs`) and the
  subscriptions see does not change: a watch still starts, reports and stops
  as it does now.

macOS keeps FSEvents streams and kqueues, whose limits are of another kind
(`tree_watch.rs`). Windows keeps a watcher for each watch: it sets no such
limit, and its watcher, asked again for a folder it watches, opens a second
reading of it and loses the first, so sharing one there would need what a
folder's followers ask of it tracked apart.

## Spike

A runner on a Linux device whose per-user limit is 128 served a
conversation whose project holds 211 skill folders, through a turn and a
second conversation on the same project:

- Before, the runner held 125 instances of one watch each, all the limit
  left, and logged `subscription.unwatchable` with "No file descriptors
  available" for each folder past them.
- After, it held one instance with 213 watches, the project, its skills
  folder and each skill's, the same with the second conversation, and none
  when idle. A skill added afterwards reached the next request's skills
  list.

## Prior art

- [@parcel/watcher](https://github.com/parcel-bundler/watcher), which VS
  Code uses, keeps one shared backend per process
  ([`Backend::getShared`](https://github.com/parcel-bundler/watcher/blob/master/src/Backend.cc)),
  whose Linux backend is one inotify instance holding every subscription's
  watches, removing a watch only when no other subscription uses it
  ([`InotifyBackend.cc`](https://github.com/parcel-bundler/watcher/blob/master/src/linux/InotifyBackend.cc)).
- [notify](https://github.com/notify-rs/notify), the crate the runner uses,
  holds any number of paths in one watcher; the runner made one watcher per
  watch.

## Alternatives

- Raise `fs.inotify.max_user_instances`: the setting is the host's, and a
  container cannot change it.
- Watch fewer folders, such as the skills folder alone and not each skill:
  the count still grows with every other watch, and a skill's file changes
  inside its own folder would go unseen.

## Open questions

None.
