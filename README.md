# Jujutsu


## Disclaimer

This is not a Google product. It is an experimental version-control system
(VCS). It was written by me, Martin von Zweigbergk (martinvonz@google.com). It
is my personal hobby project. It does not indicate any commitment or direction
from Google.


## Introduction

I started the project mostly in order to test the viability of some UX ideas in
practice. I continue to use it for that, but my short-term goal now is to make
it useful as an alternative CLI for Git repos.

The command-line tool is called `jj` for now because it's easy to type and easy
to replace (rare in English). The project is called "Jujutsu" because it matches
"jj" (I initially called it "Jujube", but changed since jujutsu is more
well-known).

Features:

 * **Compatible with Git**

   Jujutsu has two backends. One of them is a Git backend (the other is a
   native one). This lets you use Jujutsu as an alternative interface to Git.
   The commits you create will look like regular Git commits. You can always
   switch back to Git.

 * **The working copy is automatically committed**

   Most Jujutsu commands automatically commit the working copy. This leads to a
   simpler and more powerful interface, since all commands work the same way on
   the working copy or any other commit. It also means that you can always check
   out a different commit without first explicitly committing the working copy
   changes (you can even check out a different commit while resolving merge
   conflicts).

 * **Operations update the repo first, then possibly the working copy**

   The working copy is only updated at the end of an operation, after all other
   changes have already been recorded. This means that you can run any command
   (such as `jj rebase`) even if the working copy is dirty.

 * **Entire repo is under version control**

   All operations you perform in the repo are recorded, along with a snapshot of
   the repo state after the operation. This means that you can easily revert to
   an earlier repo state, or to simply undo a particular operation (which does
   not necessarily have to be the most recent operation).

 * **Conflicts can be recorded in commits**

   If an operation results in conflicts, information about those conflicts will
   be recorded in the commit(s). The operation will succeed. You can then
   resolve the conflicts later. One consequence of this design is that there's
   no need to continue interrupted operations. Instead, you get a single
   workflow for resolving conflicts, regardless of which command caused them.
   This design also lets Jujutsu rebase merge commits correctly (unlike both Git
   and Mercurial).

 * **Supports Evolution**
   Jujutsu copies the Evolution feature from Mercurial (see
   https://www.mercurial-scm.org/wiki/ChangesetEvolution). However, you will
   often not need to know that because the conflict design described above lets
   Jujutsu keep commits rebased even if there are conflicts.


## Status ##

The tool is quite feature-complete. I have almost exclusively used `jj` to
develop the project itself since early January 2021. However, there *will* be
changes to workflows and backward-incompatible changes to the on-disk formats
(I'll try to provide upgrade commands if requested). It's also likely that
workflows and setups different from what I personally use are not well
supported. 


## Getting started

The best way to get started is probably to go through
[the tuturial](docs/tutorial.md).


## Future plans

TODO
