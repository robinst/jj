# Copy Tracking and Tracing Design

Authors: [Daniel Ploch](mailto:dploch@google.com)

**Summary:** This Document documents an approach to tracking and detecting copy
information in jj repos, in a way that is compatible with both Git's detection
model and with custom backends that have more complicated tracking of copy
information. This design affects the output of diff commands as well as the
results of rebasing across remote copies.

## Objective

Add support for copy information that is sufficient for at least the following
use cases:

* Diffing: If a file has been copied, show a diff compared to the source version
  instead of showing a full addition.
* Merging: When one side of a merge has copied a file and the other side has
  modified it, propagate the changes to the other side. (There are many other
  case to handle too.)
* Log: It should be possible to run something like `jj log -p <file>` and follow
  the file backwards when it had been created by copying.
* Annotate (blame): Similar to the log use case, we should follow the file
  backwards when it had been created by copying.

The solution should support recording and retrieving copy info in a way that
is performant both for Git, which synthesizes copy info on the fly between
arbitrary trees, and for custom backends which may explicitly record and
re-serve copy info over arbitrarily large commit ranges.

The APIs should be defined in a way that makes it easy for custom backends to
ignore copy info entirely until they are ready to implement it.

## Desired UX

### New commands

We will add `jj file copy` and `jj file move` commands (tenative names) to
record copy info. As with most commands, they can be run on any commit, and they
default to running on the current working-copy commit. If the backend supports
recording copy info, then these commands will update the commit with the copy
info. Otherwise, they will have no effect (ideally not creating an unchanged
commit, and ideally telling the user that it had no effect).

### Design goals

#### Restoring from a commit should preserve copies

For example, `jj new X--; jj restore --from X` should restore any copies
made in `X-` and `X` into the new working copy. Transitive copies should
be "flattened". For example, if `X-` renamed `foo` to `bar` and `X` renamed
`bar` to `baz`, then the restored commit should rename `foo` to `baz`.

This also applies to reparenting in general, such as for
["verbatim rebase"](https://github.com/martinvonz/jj/issues/1027).

#### Diff after restore

`jj restore --from X; jj diff --from X` should be empty.

#### Lossless round-trip of rebase

Except for the `A+(A-B)=A` rule, rebasing is never currently lossy; rebasing a
commit and then rebasing it back yields the same content. We should ideally
preserve this property when possible.

For example:

```
$ jj log
C rename bar->baz
|
B rename foo->bar
|
A add foo
$ jj rebase -r C -d A
$ jj rebase -r C -d B
```

In order for that round-trip rebase to be lossless, we would presumably record
some kind of conflict in the intermediate commit.

#### Backing out parent commit should be a no-op

For example:

```
$ jj log
C rename foo->baz
|
| B rename foo->bar
|/
A add foo
$ jj rebase -r C -d B
$ jj backout -r C -d C
$ jj diff --from B # Should be empty
```

This is a special case of the lossless rebase.

#### Parallelize/serialize

This is another special case of the lossless rebase.

```
$ jj log
E edit qux
|
D rename baz->qux
|
C rename bar->baz
|
B rename foo->bar
|
A add foo
$ jj parallelize B::D
# There should be no conflict in E and it should look like a
# regular edit just like before
$ jj rebase -r C -A B
$ jj rebase -r D -A C
# Now we're back to the same graph as before.
```

#### Copies inside merge commit

We should be able to resolve a naming conflict:
```
$ jj log
D  resolve naming conflict by choosing the name `bar`
|\
C | rename foo->baz
| |
| B rename foo->bar
|/
A add foo
```

We should also be able to back out that resolution and get back into the
name-conflicted state.

We should be able to rename files that exist on only one side:
```
$ jj log
D  rename foo2->foo3 and bar2->bar3
|\
C | rename bar->bar2
| |
| B rename foo->foo2
|/
A add foo and bar
```

## Data model changes

So far, a commit has been purely a snapshot (with some metadata that doesn't
affect the content or diff in any way). When we add copy info, that is no
longer true. That's because the copy info we plan to add will indicate copies
compared to the parent(s), i.e. inherently not snapshot-based.

This has several important consequences:

* Without copy info, if there's a linear chain of commits A..D, you can find
  the total diff by diffing just D-A. That works because (B-A)+(C-B)+(D-C)
  simplifies to just D-A. However, if there is copy info, the total diff will
  involve copy info. If that's associated with the individual commits, we will
  need to aggregate it somehow.
* Restoring from another tree is no longer just a matter of copying that tree;
  we also need to figure out copies between the old tree and the new tree.
* Conflict states are currently represented by a series of tree states to add
  and remove. Because we have the individual states, a conflict like
  `A+(C-B)+(D-C)` can be simplified. With copy tracking, we would need to
  augment that somehow.
* If we have a 3-sided conflict where one patch renames foo->bar and the other
  renames bar->baz, it's not necessarily safe to chain those two together into
  foo->baz, since foo could be two different files in the two patches'
  parents. It's also possible that the bar->baz rename should come first and
  the foo->bar rename should come after.

### Proposed conflict representation

Our `MergedTree` type, which is what calculates a conflicted tree on the fly,
is currently defined by a series of positive and negative terms. We will
extend it to instead be a snapshot plus a series of diffs, where each diff
has attached copy info:

```rust
struct MergedTree {
    snapshot: Tree,
    diffs: Diff
}

struct Diff {
    before: Tree,
    after: Tree,
    /// Copies from `before` to `after`
    copies: Vec<CopyInfo>,
    /// Copies from `before` to `snapshot`
    copies_to_snapshot: Vec<CopyInfo>,
}

struct CopyInfo {
    source: RepoPathBuf,
    target: RepoPathBuf,
    // Maybe more fields here for e.g. "do not propagate"
}
```

This should be enough to be able to reproduce the state.

### Conflict flattening and simplification

#### Simplification

The tree states will be simplified as before. When a match has been found for
simplifying (chaining) tree diffs, we will also chain any copy info related to
the involved diffs. After chaining copies, any remaining copy info that has a
source that doesn't exist in the `before` tree or a target that doesn't exist in
the `after` tree will be dropped.

#### Flattening

Merge flattening is when a merge of merges is flattened into a single-level
merge. That is done by effectively adding diffs from the positive terms and
by adding reversed diffs from the negative terms.


When we add copy info, we should do the same.


#### Examples




Example:

```
D  rename foo->qux
|
| C rename bar->baz
| |
| B rename foo->bar
|/
A add foo
```

Now rebase B::C onto D. The rebased B (B') will be:

```
snapshot: D
diffs: [{
    before: A
    after: B
    copies: [foo->bar]
    copies_to_snapshot: [foo->qux]
}]
```

Rebased C before simplification will be:

```
snapshot: B'
diffs: [{
    before: B
    after: C
    copies: [bar->baz]
    copies_to_snapshot: [bar->qux]
}]
```

After expanding B':

```
snapshot: D
diffs: [{
    before: A
    after: B
    copies: [foo->bar]
    copies_to_snapshot: [foo->qux]
},{
    before: B
    after: C
    copies: [bar->baz]
    copies_to_snapshot: [bar->qux]
}]
```

After simplfication:

```
snapshot: D
diffs: [{
    before: A
    after: C
    copies: [foo->baz]
    copies_to_snapshot: [foo->qux]
}]
```

The bar->qux rename was discarded because `bar` doesn't exist in A.

Now rebase B'::C' back onto A. The rebased B' (B'') will be:

```
snapshot: A
diffs: [{
    before: D
    after: B'
    copies: [foo->bar]
    copies_to_snapshot: [foo->qux]
}]
```

After expanding B':
```
snapshot: D
diffs: [{
    before: A
    after: B
    copies: [foo->bar]
    copies_to_snapshot: [foo->qux]
}]
```

After simplification:
```
snapshot: D
diffs: [{
    before: A
    after: B
    copies: [foo->bar]
    copies_to_snapshot: [foo->qux]
}]
```


## Interface Design

### Read API

Copy information will be served both by a new Backend trait method described
below, as well as a new field on Commit objects for backends that support copy
tracking:

```rust
/// An individual copy source.
pub struct CopySource {
    /// The source path a target was copied from.
    ///
    /// It is not required that the source path is different than the target
    /// path. A custom backend may choose to represent 'rollbacks' as copies
    /// from a file unto itself, from a specific prior commit.
    path: RepoPathBuf,
    file: FileId,
    /// The source commit the target was copied from. If not specified, then the
    /// parent of the target commit is the source commit. Backends may use this
    /// field to implement 'integration' logic, where a source may be
    /// periodically merged into a target, similar to a branch, but the
    /// branching occurs at the file level rather than the repository level. It
    /// also follows naturally that any copy source targeted to a specific
    /// commit should avoid copy propagation on rebasing, which is desirable
    /// for 'fork' style copies.
    ///
    /// If specified, it is required that the commit id is an ancestor of the
    /// commit with which this copy source is associated.
    commit: Option<CommitId>,
}

pub enum CopySources {
    Resolved(CopySource),
    Conflict(HashSet<CopySource>),
}

/// An individual copy event, from file A -> B.
pub struct CopyRecord {
    /// The destination of the copy, B.
    target: RepoPathBuf,
    /// The CommitId where the copy took place.
    id: CommitId,
    /// The source of the copy, A.
    sources: CopySources,
}

/// Backend options for fetching copy records.
pub struct CopyRecordOpts {
    // TODO: Probably something for git similarity detection
}

pub type CopyRecordStream = BoxStream<BackendResult<CopyRecord>>;

pub trait Backend {
    /// Get all copy records for `paths` in the dag range `roots..heads`.
    ///
    /// The exact order these are returned is unspecified, but it is guaranteed
    /// to be reverse-topological. That is, for any two copy records with
    /// different commit ids A and B, if A is an ancestor of B, A is streamed
    /// after B.
    ///
    /// Streaming by design to better support large backends which may have very
    /// large single-file histories. This also allows more iterative algorithms
    /// like blame/annotate to short-circuit after a point without wasting
    /// unnecessary resources.
    async fn get_copy_records(&self, paths: &[RepoPathBuf], roots: &[CommitId], heads: &[CommitId]) -> CopyRecordStream;
}
```

Obtaining copy records for a single commit requires first computing the files
list for that commit, then calling get_copy_records with `heads = [id]` and
`roots = parents()`. This enables commands like `jj diff` to produce better
diffs that take copy sources into account.

### Write API

Backends that support tracking copy records at the commit level will do so
through a new field on `backend::Commit` objects:

```rust
pub struct Commit {
    ...
    copies: Option<HashMap<RepoPathBuf, CopySources>>,
}

pub trait Backend {
    /// Whether this backend supports storing explicit copy records on write.
    fn supports_copy_tracking(&self) -> bool;
}
```

This field will be ignored by backends that do not support copy tracking, and
always set to `None` when read from such backends. Backends that do support copy
tracking are required to preserve the field value always.

This API will enable the creation of new `jj` commands for recording copies:

```shell
jj cp $SRC $DEST [OPTIONS]
jj mv $SRC $DEST [OPTIONS]
```

These commands will rewrite the target commit to reflect the given move/copy
instructions in its tree, as well as recording the rewrites on the Commit
object itself for backends that support it (for backends that do not,
these copy records will be silently discarded).

Flags for the first two commands will include:

```
-r/--revision
    perform the copy or move at the specified revision
    defaults to the working copy commit if unspecified
-f
    force overwrite the destination path
--after
    record the copy retroactively, without modifying the targeted commit tree
--resolve
    overwrite all previous copy intents for this $DEST
--allow-ignore-copy
    don't error if the backend doesn't support copy tracking
--from REV
    specify a commit id for the copy source that isn't the parent commit
```

For backends which do not support copy tracking, it will be an error to use
`--after`, since this has no effect on anything and the user should know that.
The `supports_copy_tracking()` trait method is used to determine this.

An additional command is provided to deliberately discard copy info for a
destination path, possibly as a means of resolving a conflict.

```shell
jj forget-cp $DEST [-r REV]
```

## Behavioral Changes

### Rebase Changes

In general, we want to support the following use cases:

-   A rebase of an edited file A across a rename of A->B should transparently move the edits to B.
-   A rebase of an edited file A across a copy from A->B should _optionally_ copy the edits to B. A configuration option should be defined to enable/disable this behavior.
-   TODO: Others?

Using the aforementioned copy tracing API, both of these should be feasible. A
detailed approach to a specific edge case is detailed in the next section.

#### Rename of an added file

A well known and thorny problem in Mercurial occurs in the following scenario:

1.  Create a new file A
1.  Create new commits on top that make changes to file A
1.  Whoops, I should rename file A to B. Do so, amend the first commit.
1.  Because the first commit created file A, there is no rename to record; it's changing to a commit that instead creates file B.
1.  All child commits get sad on evolve

In jj, we have an opportunity to fix this because all rebasing occurs atomically
and transactionally within memory. The exact implementation of this is yet to be
determined, but conceptually the following should produce desirable results:

1.  Rebase commit A from parents [B] to parents [C]
1.  Get copy records from [D]->[B] and [D]->[C], where [D] are the common ancestors of [B] and [C]
1.  DescendantRebaser maintains an in-memory map of commits to extra copy info, which it may inject into (2). When squashing a rename of a newly created file into the commit that creates that file, DescendentRebase will return this rename for all rebases of descendants of the newly modified commit. The rename lives ephemerally in memory and has no persistence after the rebase completes.
1.  A to-be-determined algorithm diffs the copy records between [D]->[B] and [D]->[C] in order to make changes to the rebased commit. This results in edits to renamed files being propagated to those renamed files, and avoiding conflicts on the deletion of their sources. A copy/move may also be undone in this way; abandoning a commit which renames A->B should move all descendant edits of B back into A.

### Conflicts

With copy-tracking, a whole new class of conflicts become possible. These need
to be well-defined and have well documented resolution paths. Because copy info
in a commit is keyed by _destination_, conflicts can only occur at the
_destination_ of a copy, not at a source (that's called forking).

#### Split conflicts

Suppose we create commit A by renaming file F1 -> F2, then we split A. What
happens to the copy info? I argue that this is straightforward:

-   If F2 is preserved at all in the parent commit, the copy info stays on the parent commit.
-   Otherwise, the copy info goes onto the child commit.

Things get a little messier if A _also_ modifies F1, and this modification is
separated from the copy, but I think this is messy only in an academic sense and
the user gets a sane result either way. If they want to separate the
modification from the copy while still putting it in an earlier commit, they can
express this intent after with `jj cp --after --from`.

#### Merge commit conflicts

Suppose we create commit A by renaming file F1 -> F, then we create a sibling
commit B by renaming file F2 -> F. What happens when we create a merge commit
with parents A and B?

In terms of _copy info_ there is no conflict here, because C does not have copy
info and needs none, but resolving the contents of F becomes more complicated.
We need to (1) identify the greatest common ancestor of A and B (D)
(which we do anyway), and (2) invoke `get_copy_records()` on F for each of
`D::A` and `D::B` to identify the 'real' source file id for each parent. If
these are the same, then we can use that as the base for a better 3-way merge.
Otherwise, we must treat it as an add+add conflict where the base is the empty
file id.

It is possible that F1 and F2 both came from a common source file G, but that
these copies precede D. In such case, we will not produce as good of a merge
resolution as we theoretically could, but (1) this seems extremely niche and
unlikely, and (2) we cannot reasonably achieve this without implementing some
analogue of Mercurial's linknodes concept, and it would be nice to avoid that
additional complexity.

#### Squash conflicts

Suppose we create commit A by renaming file F1 -> F, then we create child
commit B in which we replace F by renaming F2 -> F. This touches on two issues.

Firstly, if these two commits are squashed together, then we have a destination
F with two copy sources, F1 and F2. In this case, we can store a
`CopySources::Conflict([F1, F2])` as the copy source for F, and treat this
commit as 'conflicted' in `jj log`. `jj status` will need modification to show
this conflicted state, and `jj resolve` will need some way of handling the
conflicted copy sources (possibly printing them in some structured text form,
and using the user's merge tool to resolve them). Alternatively, the user can
'resolve directly' by running `jj cp --after --resolve` with the desired copy
info.

Secondly, who is to say that commit B is 'replacing' F at all? In some version
control systems, it is possible to 'integrate' a file X into an existing file Y,
by e.g. propagating changes in X since its previous 'integrate' into Y, without
erasing Y's prior history in that moment for the purpose of archaeology. With
the commit metadata currently defined, it is not possible to distinguish
between a 'replacement' operation and an 'integrate' operation.

##### Track replacements explicitly

One solution is to add a `destructive: bool` field or similar to the
`CopySource` struct, to explicitly distinguish between these two types of copy
records. It then becomes possible to record a non-destructive copy using
`--after` to recognize that a file F was 'merged into' its destination, which
can be useful in handling parallel edits of F that later sync this information.

##### Always assume replacement

Alternatively, we can keep copy-tracking simple in jj by taking a stronger
stance here and treating all copies-onto-existing-files as 'replacement'
operations. This makes integrations with more complex VCSs that do support
'integrate'-style operations trickier, but it is possible that a more generic
commit extension system is better suited to such backends.

### Future Changes

An implementation of `jj blame` or `jj annotate` does not currently exist, but
when it does we'll definitely want it to be copy-tracing aware to provide
better annotations for users doing archaeology. The Read APIs provided are
expected to be sufficient for these use cases.

## Non-goals

### Tracking copies in Git

Git uses rename detection rather than copy tracking, generating copy info on
the fly between two arbitrary trees. It does not have any place for explicit
copy info that _exchanges_ with other users of the same git repo, so any
enhancements jj adds here would be local only and could potentially introduce
confusion when collaborating with other users.

### Directory copies/moves

All copy/move information will be read and written at the file level. While
`jj cp|mv` may accept directory paths as a convenience and perform the
appropriate tree modification operations, the renames will be recorded at the
file level, one for each copied/moved file.


## Alternatives considered

### Detect copies (like Git)

Git doesn't record copy info. Instead, it infers it when comparing two trees.

It seems hard to make this model scale to very large repos. By supporting
querying of copy info only between commits (not trees) as we have in the chosen
solution, we allow the backend to consider the history when calculating the
copies.

### Record file IDs in trees (BitKeeper-like model)

BitKeeper records a file ID for each path (or maybe it's a path for each file
ID). That way you can compare two arbitrary trees, find the added and deleted
files and just compare the file IDs to figure out which of them are renames.

This model doesn't seem to be easily extensible to support copies (in addition
to renames).

To perform a rebase across millions of commits, we would not want to diff the
full trees because that would be too expensive (probably millions of modified
files). We could perhaps instead find renames by bisecting to find the commit
deleted any of the files modified in the commit we're rebasing.

Another problem is how to synthesize the file IDs in the Git backend.
